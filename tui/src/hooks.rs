//! Hook management delegates policy and transactions to the shared application layer.
use crate::{screen::Screen, state::AppState, *};
#[path = "hooks_view.rs"]
mod view;
use macc_core::hooks::{Action, Status};
#[derive(Default)]
pub(crate) struct HooksScreen {
    view: Option<Status>,
    index: usize,
    scroll: u16,
    query: String,
    input: Option<(bool, String)>, // true: external manifest path; false: filter
    pending: Option<Action>,
    message: String,
}
impl HooksScreen {
    pub fn input_active(&self) -> bool {
        self.input.is_some()
    }
    pub fn modal(&self) -> bool {
        self.input.is_some() || self.pending.is_some()
    }
    pub fn refresh(&mut self, root: &std::path::Path) {
        match macc_core::hooks::execute(root, Action::Status) {
            Ok(view) => {
                self.view = Some(view);
                self.message.clear();
            }
            Err(e) => self.message = e.to_string(),
        }
    }
    fn filtered(&self) -> Vec<&macc_core::hooks::HookState> {
        self.view
            .as_ref()
            .map(|v| {
                v.hooks
                    .iter()
                    .filter(|h| {
                        format!("{} {}", h.hook.id, h.hook.description)
                            .to_lowercase()
                            .contains(&self.query.to_lowercase())
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn bindings(&self) -> Vec<(&'static str, &'static str)> {
        if self.input.is_some() {
            return vec![("Enter", "Review input"), ("Esc", "Cancel input")];
        }
        if self.pending.is_some() {
            return vec![
                ("Enter", "Confirm reviewed change"),
                ("PgUp/PgDn", "Scroll review"),
                ("Esc", "Cancel change"),
            ];
        }
        vec![
            ("↑↓", "Select hook"),
            ("PgUp/PgDn", "Scroll details"),
            ("Space", "Review activation…"),
            ("d", "Use default…"),
            ("i", "Import external hook…"),
            ("Delete", "Remove external hook…"),
            ("/", "Filter"),
            ("r", "Refresh"),
            ("Esc", "Back"),
        ]
    }
    fn key(&mut self, root: &std::path::Path, key: KeyCode) -> macc_core::Result<bool> {
        if let Some((path_input, text)) = &mut self.input {
            match key {
                KeyCode::Esc => self.input = None,
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Char(c) => {
                    if text.len() < 4096 {
                        text.push(c);
                    }
                }
                KeyCode::Enter => {
                    let (path_input, text) = (*path_input, text.clone());
                    if path_input {
                        use std::io::Read;
                        let mut bytes = Vec::new();
                        std::fs::File::open(text)
                            .map_err(|e| {
                                macc_core::MaccError::Validation(format!("Read hook manifest: {e}"))
                            })?
                            .take(262145)
                            .read_to_end(&mut bytes)
                            .map_err(|e| macc_core::MaccError::Validation(e.to_string()))?;
                        if bytes.len() > 262144 {
                            return Err(macc_core::MaccError::Validation(
                                "Hook manifest exceeds 256 KiB".into(),
                            ));
                        }
                        let definition = serde_json::from_slice(&bytes).map_err(|e| {
                            macc_core::MaccError::Validation(format!("Invalid hook manifest: {e}"))
                        })?;
                        let reviewed = macc_core::hooks::review(definition)?;
                        self.pending = Some(Action::Import {
                            sha256: reviewed.sha256,
                            definition: reviewed.definition,
                            revision: self
                                .view
                                .as_ref()
                                .map(|v| v.revision.clone())
                                .unwrap_or_default(),
                        });
                    } else {
                        self.query = text;
                        self.index = 0;
                    }
                    self.input = None;
                }
                _ => {}
            }
            return Ok(false);
        }
        if matches!(key, KeyCode::PageUp | KeyCode::PageDown) && self.input.is_none() {
            self.scroll = if key == KeyCode::PageDown {
                self.scroll.saturating_add(5)
            } else {
                self.scroll.saturating_sub(5)
            };
            return Ok(false);
        }
        if self.pending.is_some() {
            match key {
                KeyCode::Esc => self.pending = None,
                KeyCode::Enter => {
                    let action = self.pending.as_ref().unwrap().clone();
                    self.view = Some(macc_core::hooks::execute(root, action)?);
                    self.pending = None;
                    self.message = "Saved. Run Apply to refresh preparation receipts before launching the coordinator.".into();
                }
                _ => {}
            }
            return Ok(false);
        }
        if matches!(
            key,
            KeyCode::Up | KeyCode::Down | KeyCode::Char(' ' | 'd' | 'i') | KeyCode::Delete
        ) {
            self.scroll = 0;
        }
        match key {
            KeyCode::Esc => return Ok(true),
            KeyCode::Down => {
                self.index = (self.index + 1).min(self.filtered().len().saturating_sub(1))
            }
            KeyCode::Up => self.index = self.index.saturating_sub(1),
            KeyCode::Char('/') => self.input = Some((false, self.query.clone())),
            KeyCode::Char('i') => self.input = Some((true, String::new())),
            KeyCode::Char('r') => self.refresh(root),
            KeyCode::Char(' ' | 'd') | KeyCode::Delete => {
                let items = self.filtered();
                if let Some(hook) = items.get(self.index.min(items.len().saturating_sub(1))) {
                    let revision = self.view.as_ref().unwrap().revision.clone();
                    let id = hook.hook.id.clone();
                    if key == KeyCode::Delete {
                        self.pending = Some(Action::Remove { id, revision });
                    } else if key == KeyCode::Char('d') {
                        self.pending = Some(Action::Set {
                            id,
                            enabled: None,
                            revision,
                        });
                    } else if hook.enabled && !hook.required_by.is_empty() {
                        self.message = format!(
                            "Required by {}. Disable the owning feature or output contract first.",
                            hook.required_by.join(", ")
                        );
                    } else {
                        self.pending = Some(Action::Set {
                            id,
                            enabled: Some(!hook.enabled),
                            revision,
                        });
                    }
                }
            }
            _ => {}
        }
        Ok(false)
    }
}
pub(crate) fn handle(state: &mut AppState, key: KeyCode) -> bool {
    if key == KeyCode::Char('H')
        && state.current_screen() != Screen::Hooks
        && !command_catalog::exclusive(state)
    {
        if let Some(paths) = &state.project_paths {
            state.hooks.refresh(&paths.root);
        }
        state.push_screen(Screen::Hooks);
        return true;
    }
    if state.current_screen() != Screen::Hooks {
        return false;
    }
    if key == KeyCode::Char('?') && !state.hooks.input_active() {
        state.toggle_help();
        return true;
    }
    let Some(paths) = state.project_paths.clone() else {
        return true;
    };
    if state.hooks.view.is_none() {
        state.hooks.refresh(&paths.root);
    }
    let mutation = state.hooks.pending.is_some() && key == KeyCode::Enter;
    if mutation {
        let gate = state.gate_project_mutation();
        if let Err(e) = gate {
            state.hooks.message = e.to_string();
            return true;
        }
        if state.working_copy != state.config {
            state.hooks.message =
                "Save or discard other configuration edits before changing hooks.".into();
            return true;
        }
    }
    match state.hooks.key(&paths.root, key) {
        Ok(true) => state.pop_screen(),
        Ok(false) => {
            if mutation && state.hooks.pending.is_none() {
                match macc_core::load_canonical_config(&paths.config_path) {
                    Ok(saved) => {
                        state.config = Some(saved.clone());
                        state.working_copy = Some(saved);
                    }
                    Err(e) => {
                        state.hooks.message =
                            format!("Hooks saved, but reloading configuration failed: {e}")
                    }
                }
            }
        }
        Err(e) => state.hooks.message = e.to_string(),
    }
    true
}

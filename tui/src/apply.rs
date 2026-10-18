//! Apply preparation, explicit confirmation and execution through the Engine facade.
use super::*;
use crossterm::event::KeyCode;
use macc_core::resolve::MaterializedFetchUnit;
use std::sync::mpsc::{self, Receiver, TryRecvError};

#[derive(Default)]
pub(crate) struct ApplyRuntime {
    pub confirm_open: bool,
    pub confirm_selected: usize,
    pub selected: usize,
    pub scroll: u16,
    preparing: Option<Receiver<Result<Vec<MaterializedFetchUnit>, String>>>,
    planned_config: Option<CanonicalConfig>,
    planned_fingerprint: Option<String>,
}
impl AppState {
    pub(crate) fn apply_confirmation_open(&self) -> bool {
        self.apply_runtime.confirm_open
    }
    pub(crate) fn apply_preparing(&self) -> bool {
        self.apply_runtime.preparing.is_some()
    }
    pub fn open_apply_screen(&mut self) {
        if self.apply_preparing() {
            self.push_screen(Screen::Apply);
            return;
        }
        self.apply_runtime = Default::default();
        self.apply_context = None;
        self.apply_error = None;
        self.apply_feedback = None;
        self.apply_progress = None;
        self.apply_consent_input.clear();
        self.apply_user_consent_granted = false;
        let input = self.project_paths.clone().zip(self.working_copy.clone());
        match input {
            None => {
                self.apply_error = Some("Apply requires a loaded project and configuration.".into())
            }
            Some((paths, canonical)) => {
                match resolve_fetch_units(&paths, &resolve(&canonical, &CliOverrides::default())) {
                    Err(error) => self.apply_error = Some(error.to_string()),
                    Ok(units) => {
                        let (tx, rx) = mpsc::channel();
                        self.apply_runtime.planned_config = Some(canonical.clone());
                        self.apply_runtime.preparing = Some(rx);
                        // Fetching only fills the source cache. Never let progress messages enter the TUI terminal.
                        thread::spawn(move || {
                            let _ = tx.send(
                                materialize_fetch_units(
                                    &paths,
                                    units,
                                    true,
                                    canonical.settings.offline,
                                )
                                .map_err(|e| e.to_string()),
                            );
                        });
                    }
                }
            }
        }
        if self.current_screen() != Screen::Apply {
            self.push_screen(Screen::Apply);
        }
    }
    pub(crate) fn poll_apply_preparation(&mut self) {
        let Some(rx) = &self.apply_runtime.preparing else {
            return;
        };
        let result = match rx.try_recv() {
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err("Apply preparation stopped unexpectedly. Refresh the plan.".into())
            }
            Ok(result) => result,
        };
        self.apply_runtime.preparing = None;
        let result = result.and_then(|units| {
            let paths = self
                .project_paths
                .as_ref()
                .ok_or("Project closed during preparation")?;
            let canonical = self
                .apply_runtime
                .planned_config
                .as_ref()
                .ok_or("Missing plan configuration")?;
            let plan = self
                .engine
                .plan(paths, canonical, &units, &CliOverrides::default())
                .map_err(|e| e.to_string())?;
            let operations = self.engine.plan_operations(paths, &plan);
            let user_ops = operations
                .iter()
                .filter(|op| op.scope == Scope::User)
                .count();
            Ok(ApplyContext {
                project_ops: operations.len() - user_ops,
                user_ops,
                operations,
                plan,
                backup_preview: format!("{}/<timestamp>", paths.backups_dir.display()),
            })
        });
        match result {
            Ok(context) => match macc_core::service::apply_review::fingerprint(
                &context.plan,
                &context.operations,
            ) {
                Ok(fingerprint) => {
                    self.apply_runtime.planned_fingerprint = Some(fingerprint);
                    self.apply_context = Some(context);
                }
                Err(error) => self.apply_error = Some(error.to_string()),
            },
            Err(error) => self.apply_error = Some(error),
        }
    }
    pub fn append_apply_consent_char(&mut self, ch: char) {
        self.apply_consent_input.push(ch);
        self.apply_user_consent_granted = self.apply_consent_input.eq_ignore_ascii_case("YES");
    }
    pub fn pop_apply_consent_char(&mut self) {
        self.apply_consent_input.pop();
        self.apply_user_consent_granted = self.apply_consent_input.eq_ignore_ascii_case("YES");
    }
    /// Enter on the page opens a review; it never writes files.
    pub fn attempt_apply(&mut self) {
        if self.apply_context.is_none() || self.apply_preparing() {
            return;
        }
        if self.apply_feedback.is_some() {
            self.apply_error = Some("Refresh the plan before applying again.".into());
            return;
        }
        self.apply_runtime.confirm_open = true;
        self.apply_runtime.confirm_selected = 0;
        self.apply_consent_input.clear();
        self.apply_user_consent_granted = false;
    }
    pub(crate) fn apply_key(&mut self, key: KeyCode) {
        if self.apply_runtime.confirm_open {
            match key {
                KeyCode::Esc => self.apply_runtime.confirm_open = false,
                KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                    self.apply_runtime.confirm_selected ^= 1
                }
                KeyCode::Enter if self.apply_runtime.confirm_selected == 0 => {
                    self.apply_runtime.confirm_open = false
                }
                KeyCode::Enter => self.execute_confirmed_apply(),
                KeyCode::Backspace => self.pop_apply_consent_char(),
                KeyCode::Char(c)
                    if self
                        .apply_context
                        .as_ref()
                        .is_some_and(|c| c.needs_user_consent()) =>
                {
                    self.append_apply_consent_char(c)
                }
                _ => {}
            }
            return;
        }
        match key {
            KeyCode::Enter => self.attempt_apply(),
            KeyCode::Char('r') => self.open_apply_screen(),
            KeyCode::Esc | KeyCode::Char('q') => self.pop_screen(),
            KeyCode::Up => {
                self.apply_runtime.selected = self.apply_runtime.selected.saturating_sub(1)
            }
            KeyCode::Down => {
                self.apply_runtime.selected = (self.apply_runtime.selected + 1).min(
                    self.apply_context
                        .as_ref()
                        .map(|c| c.operations.len().saturating_sub(1))
                        .unwrap_or(0),
                )
            }
            KeyCode::PageDown => {
                self.apply_runtime.scroll = self.apply_runtime.scroll.saturating_add(5)
            }
            KeyCode::PageUp => {
                self.apply_runtime.scroll = self.apply_runtime.scroll.saturating_sub(5)
            }
            _ => {}
        }
    }
    fn execute_confirmed_apply(&mut self) {
        if !self.apply_runtime.confirm_open {
            return;
        }
        if self.apply_runtime.planned_config.as_ref() != self.working_copy.as_ref() {
            self.apply_error =
                Some("Configuration changed. Cancel and refresh the plan before applying.".into());
            return;
        }
        if let Err(err) = self.gate_project_mutation() {
            self.apply_error = Some(format!(
                "Apply blocked: {}",
                format_actionable_error(&err.to_string())
            ));
            self.set_status(
                UiStatusLevel::Error,
                "Apply blocked by project ownership.",
                Some(Duration::from_secs(6)),
            );
            return;
        }

        let paths = match &self.project_paths {
            Some(paths) => paths,
            None => {
                self.apply_error = Some("No project loaded for apply.".to_string());
                return;
            }
        };

        let context = match &self.apply_context {
            Some(ctx) => ctx,
            None => {
                self.apply_error =
                    Some("No apply context available. Refresh and try again.".to_string());
                return;
            }
        };

        let current_ops = self.engine.plan_operations(paths, &context.plan);
        let fingerprint =
            macc_core::service::apply_review::fingerprint(&context.plan, &current_ops);
        if !matches!(&fingerprint, Ok(current) if Some(current) == self.apply_runtime.planned_fingerprint.as_ref())
        {
            self.apply_error = Some(
                "Files changed after preview. Cancel and refresh the plan before applying.".into(),
            );
            return;
        }

        if context.needs_user_consent() && !self.apply_user_consent_granted {
            self.apply_error =
                Some("User-level operations require typing YES before applying.".to_string());
            return;
        }

        self.apply_runtime.confirm_open = false;
        let allow_user_scope = self.apply_user_consent_granted;
        let mut plan = context.plan.clone();

        let operations = context.operations.clone();
        self.apply_feedback = None;
        self.apply_error = None;
        self.apply_progress = Some(ApplyProgress {
            current: 0,
            total: operations.len(),
            path: None,
        });

        let result = {
            // For now, engine.apply doesn't support progress callback yet,
            // but we could add it to Engine trait if needed.
            self.engine.apply(paths, &mut plan, allow_user_scope)
        };

        match result {
            Ok(report) => {
                if let Err(error) = macc_core::service::context::mark_apply_completed(paths) {
                    self.apply_error = Some(format!(
                        "Changes applied, but readiness tracking failed: {error}"
                    ));
                }
                self.apply_feedback = Some(report.render_cli());
                self.notices
                    .push("TUI apply completed successfully.".to_string());
                self.set_status(
                    if self.apply_error.is_some() {
                        UiStatusLevel::Warning
                    } else {
                        UiStatusLevel::Success
                    },
                    if self.apply_error.is_some() {
                        "Apply completed; readiness tracking needs attention."
                    } else {
                        "Apply completed."
                    },
                    Some(Duration::from_secs(5)),
                );
            }
            Err(err) => {
                self.apply_feedback = None;
                self.apply_error = Some(format!("Apply failed: {}", err));
                self.set_status(
                    UiStatusLevel::Error,
                    format!("Apply failed: {}", err),
                    Some(Duration::from_secs(8)),
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "apply_tests.rs"]
mod tests;

//! Adaptive launch dialog; decisions go through the shared Engine launch service.
use super::*;
use crossterm::event::KeyCode;
use macc_core::service::launch_review::{LaunchDecision, LaunchReview};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Clear, Paragraph, Wrap},
    Frame,
};

pub(crate) struct LaunchDialog {
    pub review: LaunchReview,
    pub supervisor: bool,
    pub create_from: Option<String>,
    pub allow_dirty: bool,
    selected: usize,
    start_selected: bool,
    error: Option<String>,
    scroll: u16,
}
impl AppState {
    pub(crate) fn request_coordinator_launch(&mut self) {
        if self.is_coordinator_running() {
            self.set_status(
                UiStatusLevel::Warning,
                "Coordinator already running.",
                Some(Duration::from_secs(3)),
            );
            return;
        }
        let Some(paths) = &self.project_paths else {
            return;
        };
        match self.engine.coordinator_launch_review(paths) {
            Ok(mut review) => {
                if self.config != self.working_copy {
                    review.blockers.push(
                        "Save the configuration draft before launching the coordinator.".into(),
                    );
                }
                self.launch_dialog = Some(LaunchDialog {
                    review,
                    supervisor: true,
                    create_from: None,
                    allow_dirty: false,
                    selected: 0,
                    start_selected: false,
                    error: None,
                    scroll: 0,
                });
            }
            Err(error) => self.set_status(
                UiStatusLevel::Error,
                format!("Launch review failed: {error}"),
                Some(Duration::from_secs(8)),
            ),
        }
    }
    pub(crate) fn launch_key(&mut self, key: KeyCode) {
        if key == KeyCode::Esc {
            self.launch_dialog = None;
            return;
        }
        let Some(dialog) = &mut self.launch_dialog else {
            return;
        };
        let fields = 1
            + usize::from(dialog.review.needs_branch_creation)
            + usize::from(dialog.review.dirty_reference);
        match key {
            KeyCode::Tab | KeyCode::Down => dialog.selected = (dialog.selected + 1) % (fields + 1),
            KeyCode::BackTab | KeyCode::Up => {
                dialog.selected = (dialog.selected + fields) % (fields + 1)
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Enter if dialog.selected < fields => {
                match dialog.selected {
                    0 => dialog.supervisor = !dialog.supervisor,
                    1 if dialog.review.needs_branch_creation => {
                        let index = dialog
                            .create_from
                            .as_ref()
                            .and_then(|v| dialog.review.branch_sources.iter().position(|s| s == v));
                        let count = dialog.review.branch_sources.len() + 1;
                        let current = index.map(|i| i + 1).unwrap_or(0);
                        let next = if key == KeyCode::Left {
                            (current + count - 1) % count
                        } else {
                            (current + 1) % count
                        };
                        dialog.create_from = if next == 0 {
                            None
                        } else {
                            dialog.review.branch_sources.get(next - 1).cloned()
                        };
                    }
                    _ => dialog.allow_dirty = !dialog.allow_dirty,
                }
            }
            KeyCode::Left | KeyCode::Right => dialog.start_selected = !dialog.start_selected,
            KeyCode::PageDown => dialog.scroll = dialog.scroll.saturating_add(2),
            KeyCode::PageUp => dialog.scroll = dialog.scroll.saturating_sub(2),
            KeyCode::Enter if !dialog.start_selected => self.launch_dialog = None,
            KeyCode::Enter => {
                if !dialog.review.blockers.is_empty() {
                    return;
                }
                if dialog.review.needs_branch_creation && dialog.create_from.is_none() {
                    dialog.error = Some("Choose a source for the missing reference branch.".into());
                    return;
                }
                if dialog.review.dirty_reference && !dialog.allow_dirty {
                    dialog.error = Some(
                        "Reference branch is dirty. Clean it or explicitly allow it once.".into(),
                    );
                    return;
                }
                let decision = LaunchDecision {
                    confirmed: true,
                    revision: dialog.review.revision.clone(),
                    supervisor: dialog.supervisor,
                    create_from: dialog.create_from.clone(),
                    allow_dirty_reference: dialog.allow_dirty,
                };
                let Some(paths) = self.project_paths.clone() else {
                    return;
                };
                let engine = self.engine.clone();
                let client_id = self.client_context.client_id.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                self.launch_start_rx = Some(rx);
                self.launch_dialog = None;
                self.coordinator_pause_error = None;
                self.coordinator_pause_command = None;
                self.coordinator_pause_task_id = None;
                self.coordinator_pause_phase = None;
                self.coordinator_pause_next_action = None;
                self.set_status(
                    UiStatusLevel::Info,
                    "Starting the reviewed coordinator run…",
                    None,
                );
                thread::spawn(move || {
                    let _ = tx.send(
                        engine
                            .coordinator_launch_confirmed(&paths, &decision, Some(&client_id))
                            .map_err(|e| e.to_string()),
                    );
                });
            }
            _ => {}
        }
    }
    pub(crate) fn poll_launch_start(&mut self) {
        let Some(rx) = &self.launch_start_rx else {
            return;
        };
        let result = match rx.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("Coordinator launcher stopped unexpectedly.".into())
            }
            Ok(result) => result,
        };
        self.launch_start_rx = None;
        match result {
            Ok(()) => {
                if let Some(handle) = self.coordinator_handle() {
                    self.claim_process_ownership(handle);
                }
                self.coordinator_registration_observed = self
                    .project_paths
                    .as_ref()
                    .and_then(|paths| self.engine.process_list_running(&paths.root).ok())
                    .is_some_and(|records| {
                        records
                            .iter()
                            .any(|r| matches!(r.process.kind, ProcessKind::Coordinator))
                    });
                self.refresh_ownership_state();
                self.coordinator_running_command = Some("run".into());
                self.coordinator_running_elapsed_secs = Some(0);
                self.coordinator_last_result = Some("Reviewed coordinator run started.".into());
                if self.current_screen() == Screen::Home {
                    self.push_screen(Screen::CoordinatorLive);
                }
                self.set_status(
                    UiStatusLevel::Success,
                    "Coordinator started.",
                    Some(Duration::from_secs(3)),
                );
            }
            Err(error) => {
                self.coordinator_last_result = Some(format!("Launch failed: {error}"));
                self.set_status(
                    UiStatusLevel::Error,
                    format!("Launch failed: {error}"),
                    Some(Duration::from_secs(8)),
                );
            }
        }
    }
    pub(crate) fn draw_launch_dialog(&self, f: &mut Frame) {
        let Some(dialog) = &self.launch_dialog else {
            return;
        };
        let r = &dialog.review;
        let area = crate::ui::centered_rect(90, 95, crate::ui::content_area(f.size()));
        f.render_widget(Clear, area);
        let block = crate::ui::panel("Review coordinator launch");
        let inner = block.inner(area);
        f.render_widget(block, area);
        let parts = Layout::default()
            .constraints([
                Constraint::Length(5),
                Constraint::Min(2),
                Constraint::Length(3),
                Constraint::Length(2),
            ])
            .split(inner);
        f.render_widget(Paragraph::new(format!("Task source: {}\nReference: {}\nTools: {}\nParallel: {} · Dispatch: {}\nMonitoring: this TUI; runtime continues independently",r.task_source,r.reference_branch,r.tools.join(", "),r.max_parallel,r.max_dispatch)).wrap(Wrap{trim:false}),parts[0]);
        let mut fields = vec![format!(
            "Supervisor: {}",
            if dialog.supervisor {
                "enabled"
            } else {
                "disabled"
            }
        )];
        if r.needs_branch_creation {
            fields.push(format!(
                "Create missing reference from: {}",
                dialog.create_from.as_deref().unwrap_or("choose a source")
            ));
        }
        if r.dirty_reference {
            fields.push(format!(
                "Allow dirty reference once: {}",
                dialog.allow_dirty
            ));
        }
        let fields = fields
            .into_iter()
            .enumerate()
            .map(|(i, s)| format!("{} {s}", if dialog.selected == i { "›" } else { " " }))
            .collect::<Vec<_>>()
            .join("\n");
        f.render_widget(Paragraph::new(fields).wrap(Wrap { trim: false }), parts[1]);
        let messages = r
            .blockers
            .iter()
            .chain(r.warnings.iter())
            .chain(dialog.error.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        f.render_widget(
            Paragraph::new(messages)
                .wrap(Wrap { trim: false })
                .scroll((dialog.scroll, 0))
                .style(Style::default().fg(if r.blockers.is_empty() {
                    Color::Yellow
                } else {
                    Color::Red
                })),
            parts[2],
        );
        let ready = r.blockers.is_empty()
            && (!r.needs_branch_creation || dialog.create_from.is_some())
            && (!r.dirty_reference || dialog.allow_dirty);
        let cancel = if !dialog.start_selected {
            "[ Cancel ]"
        } else {
            "  Cancel  "
        };
        let start = if ready {
            "Start coordinator"
        } else {
            "Complete choices to start"
        };
        let start = if dialog.start_selected {
            format!("[ {start} ]")
        } else {
            format!("  {start}  ")
        };
        let buttons = ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(format!("{cancel}   "), Style::default().fg(Color::Cyan)),
            ratatui::text::Span::styled(
                start,
                Style::default().fg(if ready { Color::Cyan } else { Color::DarkGray }),
            ),
        ]);
        f.render_widget(Paragraph::new(buttons), parts[3]);
    }
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;

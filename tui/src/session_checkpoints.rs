//! Checkpoint inspection and reviewed recovery, using the same services as Web.
use crossterm::event::KeyCode;
use macc_core::{
    session_store::{self as store, MigrationReport, StoreView},
    Result,
};
use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use serde_json::Value;
use std::path::Path;
#[derive(Default)]
pub struct CheckpointPanel {
    pub open: bool,
    selected: usize,
    scroll: u16,
    details: String,
    reconciliation: Option<(String, Value)>,
    upgrade: Option<MigrationReport>,
}
impl CheckpointPanel {
    pub fn is_mutation(&self, key: KeyCode) -> bool {
        self.open && key == KeyCode::Char('a')
    }
    pub fn key(&mut self, root: &Path, key: KeyCode, view: &StoreView) -> Result<()> {
        match key {
            KeyCode::Esc | KeyCode::Char('h') => self.open = false,
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(view.checkpoints.len().saturating_sub(1))
            }
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::Char('u') => {
                self.upgrade = Some(store::checkpoints::upgrade(root, false, None)?);
                self.reconciliation = None;
                self.details = serde_json::to_string_pretty(&self.upgrade).unwrap_or_default();
            }
            KeyCode::Enter => {
                if let Some(c) = view.checkpoints.get(self.selected) {
                    self.details =
                        serde_json::to_string_pretty(&store::checkpoints::get(root, &c.id)?)
                            .unwrap_or_default();
                    self.scroll = 0;
                }
            }
            KeyCode::Char('p') => {
                if let Some(c) = view.checkpoints.get(self.selected) {
                    let plan = store::checkpoints::reconcile(root, &c.id, None)?;
                    self.details = serde_json::to_string_pretty(&plan).unwrap_or_default();
                    self.reconciliation = Some((c.id.clone(), plan));
                    self.upgrade = None;
                    self.scroll = 0;
                }
            }
            KeyCode::Char('x') => {
                self.upgrade = None;
                self.reconciliation = None;
                self.details = "Pending action discarded.".into();
            }
            KeyCode::Char('a') => {
                if let Some(plan) = self.upgrade.as_ref() {
                    let report =
                        store::checkpoints::upgrade(root, true, Some(&plan.source_sha256))?;
                    self.details = format!(
                        "Checkpoint schema upgraded. Consistent backup: {:?}",
                        report.backup
                    );
                    self.upgrade = None;
                } else if let Some((id, plan)) = self.reconciliation.as_ref() {
                    let result =
                        store::checkpoints::reconcile(root, id, plan["current_sha256"].as_str())?;
                    self.details = serde_json::to_string_pretty(&result).unwrap_or_default();
                    self.reconciliation = None;
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub fn draw(&self, f: &mut Frame, area: Rect, view: Option<&StoreView>) {
        let mut lines=vec!["Task checkpoints — ↑↓ select | Enter inspect | p preview reconciliation | a apply reviewed plan | x cancel | u preview schema upgrade | PgUp/PgDn scroll | Esc back".to_string(),"Checkpoints are evidence. They never deliver a task or approve a review. Fresh sessions restore current criteria and acknowledge task/phase.".into()];
        if let Some(view) = view {
            lines.push(format!("Checkpoint schema ready: {}. Existing SQL stores require reviewed upgrade while all runners/coordinator/supervisor are stopped.",view.checkpoints_ready));
            for (i, c) in view.checkpoints.iter().enumerate() {
                lines.push(format!(
                    "{} {} | {} | {} | {} | {} | {}",
                    if i == self.selected { ">" } else { " " },
                    c.task_id,
                    c.phase,
                    c.role,
                    c.tool_id,
                    c.validation_state,
                    c.reason
                ));
            }
        }
        lines.push(self.details.clone());
        f.render_widget(
            Paragraph::new(lines.join("\n"))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Task checkpoint recovery"),
                )
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0)),
            area,
        );
    }
}

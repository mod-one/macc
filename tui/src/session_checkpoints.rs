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
    task_scope: Option<String>,
    selected: usize,
    scroll: u16,
    details: String,
    reconciliation: Option<(String, Value)>,
    upgrade: Option<MigrationReport>,
}
impl CheckpointPanel {
    pub fn preserve_selection(&mut self, old: Option<&StoreView>, next: &StoreView) {
        let scope = &self.task_scope;
        let selected = old
            .and_then(|v| {
                v.checkpoints
                    .iter()
                    .filter(|c| scope.as_ref().is_none_or(|t| &c.task_id == t))
                    .nth(self.selected)
            })
            .map(|c| &c.id);
        self.selected = selected
            .and_then(|id| {
                next.checkpoints
                    .iter()
                    .filter(|c| scope.as_ref().is_none_or(|t| &c.task_id == t))
                    .position(|c| &c.id == id)
            })
            .unwrap_or(0);
    }
    pub fn scope_task(&mut self, task: String) {
        self.task_scope = Some(task);
        self.selected = 0;
        self.scroll = 0;
        self.details.clear();
        self.reconciliation = None;
    }
    pub fn is_mutation(&self, key: KeyCode) -> bool {
        self.open && key == KeyCode::Char('a')
    }
    pub fn key(&mut self, root: &Path, key: KeyCode, view: &StoreView) -> Result<()> {
        let rows = view
            .checkpoints
            .iter()
            .filter(|c| {
                self.task_scope
                    .as_ref()
                    .is_none_or(|task| &c.task_id == task)
            })
            .collect::<Vec<_>>();
        match key {
            KeyCode::Esc | KeyCode::Char('h') => self.open = false,
            KeyCode::Char('*') => {
                self.task_scope = None;
                self.selected = 0;
                self.reconciliation = None;
                self.details.clear();
            }
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = (self.selected + 1).min(rows.len().saturating_sub(1)),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::Char('u') => {
                self.upgrade = Some(store::checkpoints::upgrade(root, false, None)?);
                self.reconciliation = None;
                self.details = serde_json::to_string_pretty(&self.upgrade).unwrap_or_default();
            }
            KeyCode::Enter => {
                if let Some(c) = rows.get(self.selected) {
                    self.details =
                        serde_json::to_string_pretty(&store::checkpoints::get(root, &c.id)?)
                            .unwrap_or_default();
                    self.scroll = 0;
                }
            }
            KeyCode::Char('p') => {
                if let Some(c) = rows.get(self.selected) {
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
        lines.push(format!(
            "Task scope: {} | * all checkpoints",
            self.task_scope.as_deref().unwrap_or("all")
        ));
        if let Some(view) = view {
            lines.push(format!("Checkpoint schema ready: {}. Existing SQL stores require reviewed upgrade while all runners/coordinator/supervisor are stopped.",view.checkpoints_ready));
            for (i, c) in view
                .checkpoints
                .iter()
                .filter(|c| {
                    self.task_scope
                        .as_ref()
                        .is_none_or(|task| &c.task_id == task)
                })
                .enumerate()
            {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overview_scope_filters_checkpoint_rendering_and_can_be_cleared() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let mut view = store::inspect(root).unwrap();
        for task in ["TASK_A", "TASK_B"] {
            view.checkpoints.push(serde_json::from_value(serde_json::json!({"id":task,"task_id":task,"role":"performer","tool_id":"fixture","phase":"dev","reason":"observed","validation_state":"verified","created_at":1,"content_sha256":"test"})).unwrap());
        }
        let mut panel = CheckpointPanel::default();
        panel.selected = 1;
        let mut updated = view.clone();
        updated.checkpoints.reverse();
        panel.preserve_selection(Some(&view), &updated);
        assert_eq!(panel.selected, 0);
        panel.scope_task("TASK_A".into());
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 20)).unwrap();
        terminal
            .draw(|f| panel.draw(f, f.size(), Some(&view)))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("TASK_A"));
        assert!(!text.contains("TASK_B"));
        panel.key(root, KeyCode::Char('*'), &view).unwrap();
        terminal
            .draw(|f| panel.draw(f, f.size(), Some(&view)))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("TASK_B"));
    }
}

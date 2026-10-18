//! Shared usage services, exposed as an inspectable TUI panel.
use crossterm::event::KeyCode;
use macc_core::{
    session_store::{self as store, usage::UsageView, MigrationReport},
    Result,
};
use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::path::Path;
#[derive(Default)]
pub struct UsagePanel {
    pub open: bool,
    view: UsageView,
    selected: usize,
    scroll: u16,
    details: String,
    plan: Option<MigrationReport>,
}
fn count(v: Option<u64>) -> String {
    v.map(|n| n.to_string()).unwrap_or_else(|| "unknown".into())
}
impl UsagePanel {
    pub fn refresh(&mut self, root: &Path) -> Result<()> {
        self.view = store::usage::inspect(root)?;
        Ok(())
    }
    pub(crate) fn has_reviewed_plan(&self) -> bool {
        self.plan.is_some()
    }
    pub fn is_mutation(&self, key: KeyCode) -> bool {
        self.open && key == KeyCode::Char('a')
    }
    pub fn key(&mut self, root: &Path, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Esc | KeyCode::Char('t') => self.open = false,
            KeyCode::Char('r') => self.refresh(root)?,
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.view.records.len().saturating_sub(1))
            }
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::Enter => {
                if let Some(record) = self.view.records.get(self.selected) {
                    self.details = serde_json::to_string_pretty(record).unwrap_or_default();
                    self.scroll = 0;
                }
            }
            KeyCode::Char('u') => {
                self.plan = Some(store::usage::upgrade(root, false, None)?);
                self.details = serde_json::to_string_pretty(&self.plan).unwrap_or_default();
            }
            KeyCode::Char('x') => {
                self.plan = None;
                self.details = "Pending upgrade discarded.".into();
            }
            KeyCode::Char('a') => {
                if let Some(plan) = self.plan.as_ref() {
                    let report = store::usage::upgrade(root, true, Some(&plan.source_sha256))?;
                    self.details = format!(
                        "Usage storage upgraded. Consistent backup: {:?}",
                        report.backup
                    );
                    self.plan = None;
                    self.refresh(root)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub fn draw(&self, f: &mut Frame, area: Rect) {
        let mut lines=vec!["Invocation usage".into(),
            format!("Usage schema ready: {}. Unknown counters are never zero. Estimated costs are not invoices or subscription quota.",self.view.ready),
            "Known incremental totals (missing measurements excluded; costs stay separated by currency and quality):".into()];
        for t in &self.view.totals {
            lines.push(format!("{} | {} | {} | {} calls | input {} | read {} | write {} | output {} | cost {} {} ({}) | measured {} calls",t.task_id,t.tool_id,t.phase,t.invocations,count(t.counters.input_uncached),count(t.counters.cached_read),count(t.counters.cached_write),count(t.counters.output),t.counters.cost.map(|v|format!("{:.9}",v as f64/1e9)).unwrap_or_else(||"unknown".into()),t.currency.as_deref().unwrap_or(""),t.cost_quality,t.measured_invocations));
            lines.push(format!("  Coverage: input {}/{} | cache read {}/{} | cache write {}/{} | output {}/{} | cost {}/{}", count(t.coverage.input_uncached), t.invocations, count(t.coverage.cached_read), t.invocations, count(t.coverage.cached_write), t.invocations, count(t.coverage.output), t.invocations, count(t.coverage.cost), t.invocations));
        }
        lines.push("Latest 200 invocation receipts:".into());
        for (i, r) in self.view.records.iter().enumerate() {
            lines.push(format!(
                "{} {} | {} | {} | {} | {} | {}",
                if i == self.selected { ">" } else { " " },
                r.task_id,
                r.phase,
                r.tool_id,
                r.status,
                r.token_quality,
                r.measurement_source
            ));
        }
        lines.push(self.details.clone());
        f.render_widget(
            Paragraph::new(lines.join("\n"))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Invocation usage"),
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
    fn unknown_and_zero_are_distinct_and_apply_requires_a_plan() {
        assert_eq!(count(None), "unknown");
        assert_eq!(count(Some(0)), "0");
        let root = tempfile::tempdir().unwrap();
        let mut panel = UsagePanel {
            open: true,
            ..Default::default()
        };
        panel.refresh(root.path()).unwrap();
        assert!(!panel.view.ready);
        panel.key(root.path(), KeyCode::Char('a')).unwrap();
        assert!(!root.path().join(".macc/memory.db").exists());
        assert!(panel.is_mutation(KeyCode::Char('a')));
        panel.key(root.path(), KeyCode::Esc).unwrap();
        assert!(!panel.open);
    }
}

//! Responsive Apply workspace. Command hints stay in the shared footer.
use super::*;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
impl AppState {
    pub(crate) fn draw_apply(&self, f: &mut Frame, area: Rect) {
        let parts = Layout::default()
            .constraints([Constraint::Length(5), Constraint::Min(3)])
            .split(area);
        let summary = if self.apply_runtime.applying.is_some() {
            format!("Applying and preparing enabled features…\n{}\nFollow any account setup link in the output below.",self.apply_runtime.assistant.report.as_ref().and_then(|r|r.current.as_deref()).unwrap_or("Starting preparation"))
        } else if self.apply_preparing() {
            "Preparing plan…\nFetching selected sources into the cache. Project files have not been applied.\nYou can leave this page while preparation finishes.".into()
        } else if let Some(context) = &self.apply_context {
            format!(
                "{} operations · {} project · {} user\nBackup: {}\n{}",
                context.operations.len(),
                context.project_ops,
                context.user_ops,
                context.backup_preview,
                if self.apply_feedback.is_some() {
                    "Completed. Refresh to prepare another plan."
                } else {
                    "Review the operations, then open the confirmation to apply."
                }
            )
        } else {
            "No plan available. Refresh to retry preparation.".into()
        };
        f.render_widget(
            Paragraph::new(summary)
                .wrap(Wrap { trim: false })
                .block(crate::ui::panel("Apply · review before writing")),
            parts[0],
        );
        let panes = Layout::default()
            .direction(if area.width >= 100 {
                Direction::Horizontal
            } else {
                Direction::Vertical
            })
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(parts[1]);
        let operations = self
            .apply_context
            .as_ref()
            .map(|c| c.operations.as_slice())
            .unwrap_or_default();
        let rows = operations
            .iter()
            .map(|op| {
                ListItem::new(format!(
                    "[{}] {:?}  {}",
                    if op.scope == Scope::Project {
                        "project"
                    } else {
                        "user"
                    },
                    op.kind,
                    op.path
                ))
            })
            .collect::<Vec<_>>();
        let mut selected = ListState::default();
        if !rows.is_empty() {
            selected.select(Some(self.apply_runtime.selected.min(rows.len() - 1)));
        }
        let theme = crate::ui::theme();
        f.render_stateful_widget(
            List::new(rows)
                .block(crate::ui::panel(
                    "Planned operations · select to inspect full path",
                ))
                .highlight_symbol("› ")
                .highlight_style(Style::default().bg(theme.highlight_bg)),
            panes[0],
            &mut selected,
        );
        let selected_path = operations
            .get(self.apply_runtime.selected)
            .map(|op| op.path.as_str())
            .unwrap_or("No operation selected.");
        let result = self
            .apply_error
            .as_deref()
            .or(self.apply_feedback.as_deref())
            .unwrap_or(
                "No changes applied. Confirmation is required for project and user operations.",
            );
        let result = self
            .apply_runtime
            .assistant
            .report
            .as_ref()
            .and_then(|r| r.live_output.as_deref())
            .filter(|_| self.apply_runtime.applying.is_some())
            .unwrap_or(result);
        f.render_widget(
            Paragraph::new(format!(
                "{}\n\nSelected path: {selected_path}",
                result.trim_start()
            ))
            .wrap(Wrap { trim: false })
            .scroll((self.apply_runtime.scroll, 0))
            .block(crate::ui::panel("Details / result"))
            .style(Style::default().fg(if self.apply_error.is_some() {
                Color::Red
            } else {
                theme.accent_dim
            })),
            panes[1],
        );
    }
    pub(crate) fn draw_apply_confirmation(&self, f: &mut Frame) {
        if self.current_screen() != Screen::Apply {
            return;
        }
        if self.apply_runtime.assistant.open {
            self.draw_apply_assistant(f);
            return;
        }
        if !self.apply_confirmation_open() {
            return;
        }
        let area = crate::ui::centered_rect(85, 85, crate::ui::content_area(f.size()));
        f.render_widget(Clear, area);
        let block = crate::ui::panel("Confirm Apply");
        let inner = block.inner(area);
        f.render_widget(block, area);
        let parts = Layout::default()
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(inner);
        let context = self.apply_context.as_ref();
        let mut text = context
            .map(|c| {
                format!(
                    "Apply {} operations?\nProject: {}  ·  User: {}\nBackup: {}\n\n",
                    c.operations.len(),
                    c.project_ops,
                    c.user_ops,
                    c.backup_preview
                )
            })
            .unwrap_or_default();
        if let Some(activation) = context.and_then(|c| c.plan.o2_activation.as_ref()) {
            text.push_str(&format!(
                "O2: {}. Migration backups are retained under .macc/state.\n\n",
                activation.operation
            ));
        }
        if let Some(preparation) = context.and_then(|c| c.plan.preparation.as_ref()) {
            for step in &preparation.steps {
                text.push_str(&format!("• {step}\n"));
            }
            if preparation.network {
                text.push_str(
                    "Preparation may download pinned packages or run declared tool installers.\n",
                );
            }
        }
        if context.is_some_and(|c| c.needs_user_consent()) {
            text.push_str(&format!(
                "These operations also change user files. Type YES to allow them.\nConsent: {}_\n",
                self.apply_consent_input
            ));
        } else {
            text.push_str(
                "This writes the reviewed project files. Cancel leaves them unchanged.\n",
            );
        }
        if let Some(error) = &self.apply_error {
            text.push_str(&format!("\n{error}"));
        }
        f.render_widget(
            Paragraph::new(text)
                .scroll((self.apply_runtime.confirm_scroll, 0))
                .wrap(Wrap { trim: false }),
            parts[0],
        );
        let mut buttons = Vec::new();
        for (index, label) in ["Cancel", "Apply changes"].iter().enumerate() {
            buttons.push(ratatui::text::Span::styled(
                format!("  {label}  "),
                if self.apply_runtime.confirm_selected == index {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ));
        }
        f.render_widget(
            Paragraph::new(Line::from(buttons)).block(crate::ui::panel("Choose an action")),
            parts[1],
        );
    }
}

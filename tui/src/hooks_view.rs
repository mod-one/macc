//! Hook catalogue and review presentation.
use super::*;
impl HooksScreen {
    pub fn draw(&self, f: &mut Frame, area: Rect) {
        let parts = Layout::default()
            .constraints([
                Constraint::Length(4),
                Constraint::Percentage(42),
                Constraint::Min(6),
            ])
            .split(area);
        f.render_widget(wrapped_paragraph(format!("Hooks · MACC processors and native tool integration\nEnabled means selected; installation and execution are shown separately below.\nFilter: {} · select a hook to inspect deployment",self.query), "Hooks"),parts[0]);
        let items = self.filtered();
        let selected = self.index.min(items.len().saturating_sub(1));
        let mut selection = ListState::default();
        selection.select(Some(selected));
        f.render_stateful_widget(
            List::new(
                items
                    .iter()
                    .map(|h| {
                        ListItem::new(format!(
                            "[MACC {}] {}{}",
                            if h.enabled { "on" } else { "off" },
                            h.hook.name,
                            if h.required_by.is_empty() {
                                ""
                            } else {
                                " · required"
                            }
                        ))
                    })
                    .collect::<Vec<_>>(),
            )
            .block(panel("Available hooks"))
            .highlight_symbol("› ")
            .highlight_style(Style::default().fg(theme().accent)),
            parts[1],
            &mut selection,
        );
        let mut details = if let Some(h) = items.get(selected) {
            format!("{}\n{}\nVersion: {} · Event: {:?}\nOwners: {}\nManual override: {:?}\nSource: {} / {} ({})\nExplicit skill contracts: {}\n{}",h.hook.id,h.hook.description,h.hook.version,h.hook.event,if h.required_by.is_empty() {"None; user configurable".into()} else {h.required_by.join(", ")},h.manual,h.hook.source.repository,h.hook.source.subpath,h.hook.source.distribution,h.hook.skills.join(", "),h.integration)
        } else {
            "No matching hook. Use [i] to import a reviewed hook.json snapshot.".into()
        };
        if let Some(view) = &self.view {
            details.push_str("\n\nNative tool deployments (output hooks only):");
            for native in &view.native {
                details.push_str(&format!("\n{}: supported={} | configured={} | installed={} | local verification={} | execution observed={}\n{}\nLast event reducers: {}",native.tool,native.supported,native.configured,native.installed,native.locally_verified,native.execution_observed,native.detail,native.applied.join(", ")));
            }
        }
        details.push_str(&format!("\n\n{}", self.message));
        f.render_widget(
            wrapped_paragraph(details, "Hook details").scroll((self.scroll, 0)),
            parts[2],
        );
        if let Some((path, text)) = &self.input {
            let popup = ui::centered_rect(80, 35, area);
            f.render_widget(Clear, popup);
            f.render_widget(
                wrapped_paragraph(
                    format!(
                        "{}\n\n{}▏",
                        if *path {
                            "Local hook.json path (import stays disabled until enabled)"
                        } else {
                            "Filter hooks by name or description"
                        },
                        text
                    ),
                    "Hook input",
                )
                .block(panel("Hook input")),
                popup,
            );
        }
        if let Some(action) = &self.pending {
            let text = match action {
                Action::Set { id,enabled,.. } => format!("Hook: {id}\nActivation: {}",enabled.map(|v| if v {"Enabled"} else {"Disabled"}).unwrap_or("Restore default; feature claims preserved")),
                Action::Import { definition,sha256,.. } => format!("Import {} v{}\n{}\nEvent: {:?}\nSkill contracts: {}\nHandler: {:?}\nSHA-256: {sha256}\nStored as an offline snapshot; initially disabled.",definition.id,definition.version,definition.description,definition.event,definition.skills.join(", "),definition.handler),
                Action::Remove { id,.. } => format!("Remove external snapshot {id} and its manual override."),
                Action::Status => String::new(),
            };
            let popup = ui::centered_rect(88, 70, area);
            f.render_widget(Clear, popup);
            f.render_widget(
                wrapped_paragraph(
                    format!(
                        "{text}\n\n{}\n\n[Enter] Confirm    [Esc] Cancel",
                        self.message
                    ),
                    "Review hook change",
                )
                .block(panel("Review hook change"))
                .scroll((self.scroll, 0)),
                popup,
            );
        }
    }
}

//! Hook catalogue and review presentation.
use super::*;
impl HooksScreen {
    pub fn draw(&self, f: &mut Frame, area: Rect) {
        let sections = Layout::default()
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(area);
        workspace_ui::tab_line(
            f,
            sections[0],
            &["Processors", "Native deployments"],
            usize::from(self.native_tab),
            theme().configure,
        );
        let (list, inspector) = workspace_ui::columns(sections[1]);
        if self.native_tab {
            let deployments = self
                .view
                .as_ref()
                .map(|v| v.native.as_slice())
                .unwrap_or_default();
            let selected = self.native_index.min(deployments.len().saturating_sub(1));
            let rows = deployments
                .iter()
                .map(|d| {
                    vec![
                        d.tool.to_string(),
                        if d.installed {
                            "Installed"
                        } else if d.configured {
                            "Configured"
                        } else if d.supported {
                            "Available"
                        } else {
                            "Unsupported"
                        }
                        .into(),
                    ]
                })
                .collect();
            workspace_ui::table(
                f,
                list,
                "Native deployments",
                &["Tool", "State"],
                rows,
                Some(selected),
                crate::navigation::Mode::Configure,
            );
            let detail = deployments.get(selected).map(|d|{
                let status = |v| if v {"Yes"} else {"No"};
                format!("{}\n\nConfigured   {}\nInstalled    {}\nVerified     {}\nObserved     {}\n\n{}\n\nLast event reducers: {}",d.tool,status(d.configured),status(d.installed),status(d.locally_verified),status(d.execution_observed),d.detail,d.applied.join(", "))
            }).unwrap_or_else(|| "No native deployments reported.".into());
            workspace_ui::details(
                f,
                inspector,
                "Deployment evidence",
                format!("{detail}\n{}", self.message),
                self.scroll,
            );
            return;
        }
        let items = self.filtered();
        let selected = self.index.min(items.len().saturating_sub(1));
        let mut selection = ListState::default();
        selection.select(Some(selected));
        f.render_stateful_widget(
            List::new(
                items
                    .iter()
                    .map(|h| {
                        ListItem::new(Line::from(vec![
                            Span::styled(
                                if h.enabled { "● On   " } else { "○ Off  " },
                                Style::default().fg(if h.enabled {
                                    theme().good
                                } else {
                                    theme().muted
                                }),
                            ),
                            Span::raw(h.hook.name.clone()),
                            Span::styled(
                                if h.required_by.is_empty() {
                                    ""
                                } else {
                                    " · required"
                                },
                                Style::default().fg(theme().configure),
                            ),
                        ]))
                    })
                    .collect::<Vec<_>>(),
            )
            .block(crate::ui::focus_panel(
                &format!(
                    "MACC processors · {}{}",
                    items.len(),
                    if self.query.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", self.query)
                    }
                ),
                crate::navigation::Mode::Configure,
            ))
            .highlight_symbol("› ")
            .highlight_style(Style::default().bg(theme().configure_selection)),
            list,
            &mut selection,
        );
        let mut details = if let Some(h) = items.get(selected) {
            let owners = if h.required_by.is_empty() {
                "User configurable".into()
            } else {
                h.required_by.join(", ")
            };
            let manual = h
                .manual
                .map(|v| if v { "On" } else { "Off" })
                .unwrap_or("Default");
            let mut detail=format!("{}   v{}\n{}\n\nEvent     {:?}\nOwner     {owners}\nOverride  {manual}\nSource    {} / {} ({})\n\n{}", h.hook.name,h.hook.version,h.hook.description,h.hook.event,h.hook.source.repository,h.hook.source.subpath,h.hook.source.distribution,h.integration);
            if !h.hook.skills.is_empty() {
                detail.push_str(&format!("\nSkills    {}", h.hook.skills.join(", ")));
            }
            detail
        } else {
            "No matching hook. Use [i] to import a reviewed hook.json snapshot.".into()
        };
        details.push_str("\n\nOpen Native deployments for installation and execution evidence.");
        details.push_str(&format!("\n\n{}", self.message));
        f.render_widget(
            wrapped_paragraph(details, "Hook details").scroll((self.scroll, 0)),
            inspector,
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

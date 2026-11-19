//! Tool inventory, field editor and installation evidence share one workspace.
use super::*;
pub(super) fn draw(f: &mut Frame, state: &AppState, area: Rect) {
    let theme = theme();
    let focused = state.current_screen() == Screen::ToolSettings;
    let installation =
        !focused && (state.tool_installation_tab || state.is_tool_install_confirmation_open());
    let sections = Layout::default()
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    workspace_ui::tab_line(
        f,
        sections[0],
        &["Settings", "Installation"],
        usize::from(installation),
        theme.configure,
    );
    let compact = sections[1].width < 90;
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(if compact && focused {
                0
            } else if compact && !installation {
                sections[1].width
            } else {
                (sections[1].width / 3).min(35)
            }),
            Constraint::Min(0),
        ])
        .split(sections[1]);
    let inventory_area = panes[0];
    let detail_area = panes[1];
    let enabled_tools = state
        .working_copy
        .as_ref()
        .map(|c| c.tools.enabled.clone())
        .unwrap_or_default();

    let mut list_state = ListState::default();
    let visible = state.filtered_tool_indices();
    if !visible.is_empty() {
        let idx = if visible.contains(&state.selected_tool_index) {
            state.selected_tool_index
        } else {
            visible[0]
        };
        let selected_pos = visible.iter().position(|v| *v == idx).unwrap_or(0);
        list_state.select(Some(selected_pos));
    }

    let items: Vec<ListItem> = visible
        .iter()
        .map(|index| {
            let tool = &state.tool_descriptors[*index];
            let is_enabled = enabled_tools.contains(&tool.id.to_string());
            let enabled_marker = workspace_ui::switch(is_enabled);
            let status = state
                .tool_checks
                .iter()
                .find(|tc| tc.tool_id.as_deref() == Some(tool.id.as_str()))
                .map(|tc| &tc.status);
            let (status_label, status_color) = installation_state(status);
            let install_hint = if matches!(status, Some(macc_core::doctor::ToolStatus::Missing))
                && tool.install.is_some()
            {
                " install"
            } else {
                ""
            };

            ListItem::new(Line::from(vec![
                Span::styled(
                    enabled_marker,
                    Style::default().fg(if is_enabled { theme.good } else { theme.muted }),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("{:<9}", status_label),
                    Style::default().fg(status_color),
                ),
                Span::raw(" "),
                Span::raw(format!("{}{}", tool.title, install_hint)),
            ]))
        })
        .collect();

    let title = format!(
        "Tools ({}/{}, shown {})",
        enabled_tools.len(),
        state.tool_descriptors.len(),
        visible.len()
    );
    let list = List::new(items)
        .block(if focused {
            panel(&title)
        } else {
            ui::focus_panel(&title, navigation::Mode::Configure)
        })
        .highlight_symbol("› ")
        .highlight_style(Style::default().bg(theme.configure_selection));
    f.render_stateful_widget(list, inventory_area, &mut list_state);

    if detail_area.is_empty() {
        return;
    }
    if installation {
        if visible.is_empty() {
            f.render_widget(
                Paragraph::new("No matching tools. Press '/' to edit search.")
                    .block(Block::default().borders(Borders::ALL)),
                detail_area,
            );
        } else {
            let selected = if visible.contains(&state.selected_tool_index) {
                state.selected_tool_index
            } else {
                visible[0]
            };
            let tool = &state.tool_descriptors[selected];
            let status = state
                .tool_checks
                .iter()
                .find(|tc| tc.tool_id.as_deref() == Some(tool.id.as_str()))
                .map(|tc| &tc.status);
            let (status_label, _) = installation_state(status);
            let is_enabled = enabled_tools.contains(&tool.id);
            let mut detail = tool_detail_text(
                tool.id.as_str(),
                tool.title.as_str(),
                tool.description.as_str(),
                tool.fields.len(),
                status_label,
                is_enabled,
            );
            detail.push_str("\nResetting a local cooldown does not renew provider quota.\n");
            detail.push_str(&tool_cooldowns::label(
                &state
                    .coordinator_throttled_tools
                    .iter()
                    .filter(|entry| entry.tool_id == tool.id)
                    .cloned()
                    .collect::<Vec<_>>(),
            ));
            if matches!(status, Some(macc_core::doctor::ToolStatus::Missing)) {
                if let Some(install) = &tool.install {
                    detail.push_str("\nInstall:\n");
                    detail.push_str(&install.confirm_message);
                }
            }
            if matches!(status, Some(macc_core::doctor::ToolStatus::Missing))
                && tool.install.is_none()
            {
                detail.push_str("\nInstall:\n");
                detail.push_str("No guided install is available for this tool.");
            }
            if let Some(macc_core::doctor::ToolStatus::Error(msg)) = status {
                detail.push_str("\nError:\n");
                detail.push_str(msg);
            }
            if state.is_tool_install_confirmation_open() {
                detail
                    .push_str("\n\nInstall confirmation pending: Enter to install, Esc to cancel.");
            }

            let detail_para = Paragraph::new(detail)
                .block(panel("Details"))
                .wrap(Wrap { trim: false });
            f.render_widget(detail_para, detail_area);
        }
    } else {
        let body_area = detail_area;
        let (left, right) = workspace_ui::columns(detail_area);
        let body_chunks = [left, right];
        let descriptor = if focused {
            state.current_tool_descriptor()
        } else {
            state.selected_tool_descriptor()
        };
        if let Some(desc) = descriptor {
            let mut list_state = ListState::default();
            if !desc.fields.is_empty() {
                let idx = state.tool_field_index.min(desc.fields.len() - 1);
                list_state.select(Some(idx));
            }

            let items: Vec<ListItem> = desc
                .fields
                .iter()
                .enumerate()
                .map(|(i, field)| {
                    let is_selected = i == state.tool_field_index;
                    let value = if focused && is_selected && state.is_tool_field_editing() {
                        format!("{}_", state.tool_field_input)
                    } else {
                        state.tool_field_display_value(field)
                    };
                    ListItem::new(Line::from(vec![
                        Span::raw(format!("{:<22}", field.label)),
                        Span::raw(" "),
                        Span::raw(value),
                    ]))
                })
                .collect();

            let settings_title = format!("{} Settings", desc.title);
            let list = List::new(items)
                .block(if focused {
                    ui::focus_panel(&settings_title, navigation::Mode::Configure)
                } else {
                    panel(&settings_title)
                })
                .highlight_symbol("› ")
                .highlight_style(Style::default().bg(theme.configure_selection));
            f.render_stateful_widget(list, body_chunks[0], &mut list_state);

            let mut detail = String::new();
            if let Some(field) = desc.fields.get(state.tool_field_index) {
                detail.push_str(&format!("Field: {}\n", field.id));
                detail.push_str(&format!("Pointer: {}\n", field.path));
                detail.push_str(&format!("Kind: {}\n", field_kind_label(&field.kind)));
                if let Some(default) = field_default_label(&field.default) {
                    detail.push_str(&format!("Default: {}\n", default));
                }
                if let FieldKind::Enum(options) = &field.kind {
                    detail.push_str(&format!("Options: {}\n", options.join(", ")));
                }
                detail.push_str("\nHelp:\n");
                detail.push_str(&field.help);
            } else {
                detail.push_str("No field selected.");
            }

            if let Some(validation) = focused
                .then(|| state.current_tool_field_validation())
                .flatten()
            {
                detail.push_str("\n\nValidation:\n");
                detail.push_str(&validation);
            }
            let detail_para = wrapped_paragraph(detail, "Field Info");
            f.render_widget(detail_para, body_chunks[1]);
        } else {
            let body = wrapped_paragraph("No tool selected. Return to Tools.", "Tool Settings");
            f.render_widget(body, body_area);
        }
    }
}

fn installation_state(status: Option<&macc_core::doctor::ToolStatus>) -> (&'static str, Color) {
    let theme = theme();
    match status {
        Some(macc_core::doctor::ToolStatus::Installed) => ("installed", theme.good),
        Some(macc_core::doctor::ToolStatus::Missing) => ("missing", theme.warn),
        Some(macc_core::doctor::ToolStatus::Error(_)) => ("error", theme.bad),
        None => ("? Unchecked", theme.warn),
    }
}

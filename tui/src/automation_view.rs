use super::*;
/// Visible order, paired with the field catalogue's tab IDs.
pub(super) const SECTIONS: &[(usize, &str)] = &[
    (1, "Run"),
    (2, "Tool routing"),
    (3, "Phases"),
    (4, "Reliability"),
    (6, "Supervisor"),
    (7, "Decisions"),
    (5, "Advanced"),
];
fn setting_value(state: &AppState, source: u8, index: usize) -> String {
    let raw = state.config_field_value(source, index);
    if source == 1 {
        match index {
            8 | 10..=12 | 18..=21 | 26 | 27 | 30 => return format!("{raw} s"),
            15 | 16 => return format!("{raw} ms"),
            _ => {}
        }
    }
    if source == 1 && index == 38 {
        return if raw == "true" {
            "● On · blocks"
        } else {
            "○ Off · warns"
        }
        .into();
    }
    if let Some(saved) = raw.strip_suffix(" [CLI OVERRIDE ACTIVE]") {
        return format!(
            "{} [CLI OVERRIDE ACTIVE]",
            workspace_ui::display_setting(saved.into())
        );
    }
    workspace_ui::display_setting(raw)
}

pub(super) fn draw(f: &mut Frame, state: &AppState, body_area: Rect, draft: &[(String, String)]) {
    let theme = theme();
    // ── Unified tabbed configuration screen ───────────────────────────
    // Layout: tab bar (1 line) + two-column body (55% list / 45% detail).
    let outer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(u16::from(state.config_tab_index != 0)),
            Constraint::Min(0),
        ])
        .split(body_area);

    // ── Tab bar ───────────────────────────────────────────────────────
    if state.config_tab_index != 0 {
        let labels: Vec<_> = SECTIONS.iter().map(|&(_, label)| label).collect();
        let selected = SECTIONS
            .iter()
            .position(|&(id, _)| id == state.config_tab_index)
            .unwrap_or(0);
        workspace_ui::tab_line(f, outer_chunks[0], &labels, selected, theme.configure);
    }
    // ── Two-column body ───────────────────────────────────────────────
    let (settings, inspector) = workspace_ui::columns(outer_chunks[1]);
    let body_chunks = [settings, inspector];

    // ── Field list ────────────────────────────────────────────────────
    let label_width = usize::from(body_chunks[0].width.saturating_sub(6))
        .saturating_mul(3)
        .div_ceil(5)
        .clamp(8, 40);
    let edited: Vec<_> = draft
        .iter()
        .filter_map(|(path, _)| crate::workbench::changes::field(path))
        .collect();
    let tab_fields = AppState::config_tab_fields(state.config_tab_index);
    let mut list_state = ListState::default();
    list_state.select(Some(
        state
            .config_view_index
            .min(tab_fields.len().saturating_sub(1)),
    ));

    let items: Vec<ListItem> = tab_fields
        .iter()
        .enumerate()
        .map(|(view_i, &(source, field_idx))| {
            let label = state.config_field_label(source, field_idx);
            let is_selected = view_i == state.config_view_index;

            let value = if is_selected {
                if let Some(input) = state.config_field_input_display() {
                    // Text-editing cursor active for this field
                    input
                } else if source == 1 && field_idx == 3 && state.tool_priority_editor_active {
                    if state.tool_priority_editor_grabbed {
                        "[GRABBED — ↑↓ to move, Space to drop]".to_string()
                    } else {
                        "[reorder — ↑↓ navigate, Space to grab]".to_string()
                    }
                } else if source == 1 && field_idx == 4 && state.tool_parallel_editor_active {
                    "[edit — ↑↓ select, ←→ adjust, g global, Enter done]".to_string()
                } else {
                    setting_value(state, source, field_idx)
                }
            } else {
                setting_value(state, source, field_idx)
            };

            // Highlight phase-override fields in warning color
            let has_override =
                source == 1 && state.phase_override_notice_for_field(field_idx).is_some();
            let changed = edited.contains(&(source, field_idx));
            let value_style = if has_override {
                Style::default().fg(theme.warn)
            } else if changed {
                Style::default().fg(theme.draft)
            } else if is_selected
                && ((source == 1 && field_idx == 3 && state.tool_priority_editor_active)
                    || (source == 1 && field_idx == 4 && state.tool_parallel_editor_active))
            {
                Style::default().fg(theme.configure)
            } else {
                Style::default().fg(theme.glyph_color(&value).unwrap_or(theme.foreground))
            };

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(
                        "{:<width$}",
                        ui::truncate_middle(label, label_width),
                        width = label_width
                    ),
                    Style::default().fg(theme.muted),
                ),
                Span::raw(" "),
                Span::styled(value, value_style),
                Span::styled(
                    if changed { " ◆" } else { "" },
                    Style::default().fg(theme.draft),
                ),
            ]))
        })
        .collect();

    let tab_name = AppState::CONFIG_TAB_NAMES
        .get(state.config_tab_index)
        .copied()
        .unwrap_or("Settings");
    let list = List::new(items)
        .block(ui::focus_panel(tab_name, navigation::Mode::Configure))
        .highlight_symbol("› ")
        .highlight_style(Style::default().bg(theme.configure_selection));
    f.render_stateful_widget(list, body_chunks[0], &mut list_state);

    // ── Detail pane ───────────────────────────────────────────────────
    // Special editors override the detail pane entirely.
    let detail_para = if state.tool_priority_editor_active {
        let list = state.tool_priority_ordered_list();
        let sel = state.tool_priority_editor_index;
        let grabbed = state.tool_priority_editor_grabbed;
        let title = if grabbed {
            "Tool Priority — GRABBED"
        } else {
            "Tool Priority — Reorder"
        };
        let mut lines: Vec<String> = if grabbed {
            vec![
                "Tool grabbed — moves with ↑/↓.".to_string(),
                String::new(),
                "↑/k  Move up".to_string(),
                "↓/j  Move down".to_string(),
                "Space/Enter  Drop here".to_string(),
                "s    Save and exit".to_string(),
                "Esc  Release grab".to_string(),
            ]
        } else {
            vec![
                "Navigate with ↑/↓, Space to grab.".to_string(),
                String::new(),
                "↑/k  Cursor up".to_string(),
                "↓/j  Cursor down".to_string(),
                "Space  Grab tool".to_string(),
                "Enter/s  Save and exit".to_string(),
                "Esc  Cancel".to_string(),
            ]
        };
        lines.push(String::new());
        for (i, tool) in list.iter().enumerate() {
            let cursor = if i == sel {
                if grabbed {
                    "✦ "
                } else {
                    "› "
                }
            } else {
                "  "
            };
            lines.push(format!("{}{}. {}", cursor, i + 1, tool));
        }
        wrapped_paragraph(lines.join("\n"), title)
    } else if state.tool_parallel_editor_active {
        let enabled = state
            .working_copy
            .as_ref()
            .map(|wc| wc.tools.enabled.clone())
            .unwrap_or_default();
        let counts: std::collections::BTreeMap<String, usize> = state
            .working_copy
            .as_ref()
            .and_then(|wc| wc.automation.coordinator.as_ref())
            .map(|c| c.max_parallel_per_tool.clone())
            .unwrap_or_default();
        let sel = state.tool_parallel_editor_index;
        let mut lines = vec![
            "Max Parallel Per Tool".to_string(),
            String::new(),
            "↑/k  Previous tool".to_string(),
            "↓/j  Next tool".to_string(),
            "←/−  Decrease (0 stops new task assignments)".to_string(),
            "→/+  Increase".to_string(),
            "g    Use global limit".to_string(),
            "Enter/s  Exit editor".to_string(),
            String::new(),
        ];
        for (i, tool) in enabled.iter().enumerate() {
            let count = match counts.get(tool) {
                None => "Global limit".to_owned(),
                Some(0) => "No new assignments".to_owned(),
                Some(limit) => limit.to_string(),
            };
            let marker = if i == sel { "› " } else { "  " };
            lines.push(format!("{}{}: {}", marker, tool, count));
        }
        wrapped_paragraph(lines.join("\n"), "Parallel Per Tool Editor")
    } else if matches!(state.current_config_field(), Some((1, 0))) {
        // Coordinator tool field — show available tools for cycling
        let enabled = state
            .working_copy
            .as_ref()
            .map(|wc| wc.tools.enabled.clone())
            .unwrap_or_default();
        let current = state.automation_field_display_value(0);
        let override_prefix = state
            .phase_override_notice_for_field(0)
            .map(|n| format!("⚠ {n}\n\n"))
            .unwrap_or_default();
        let mut lines = vec![
            format!("{}Field: Coordinator Tool", override_prefix),
            String::new(),
            state.automation_field_help(0).to_string(),
            String::new(),
            format!(
                "Current: {}",
                if current.is_empty() {
                    "(auto-select)"
                } else {
                    &current
                }
            ),
            String::new(),
            "Available tools:".to_string(),
            "  (empty)  — auto-select".to_string(),
        ];
        for t in &enabled {
            let marker = if t == &current { " ✓ " } else { "   " };
            lines.push(format!("{}{}", marker, t));
        }
        lines.push(String::new());
        lines.push("←/→ or Space/Enter to cycle".to_string());
        lines.push("s — Save to .macc/macc.yaml".to_string());
        let mut text = coordinator_basics_text(state);
        text.push_str("\n\n");
        text.push_str(&lines.join("\n"));
        wrapped_paragraph(text, "Inspector")
    } else if let Some((3, index)) = state.current_config_field() {
        wrapped_paragraph(state.decision_field_info(index), "Decisions settings")
    } else if let Some((2, index)) = state.current_config_field() {
        wrapped_paragraph(
            state
                .supervisor_commands_info()
                .unwrap_or_else(|| state.supervisor_field_info(index)),
            "Supervisor settings",
        )
    } else {
        // Default: show field description and current value.
        let detail_text = if let Some((source, field_idx)) = state.current_config_field() {
            let override_prefix = if source == 1 {
                state
                    .phase_override_notice_for_field(field_idx)
                    .map(|n| format!("⚠ {n}\n\n"))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let raw_label = if source == 0 {
                state.settings_field_label(field_idx)
            } else {
                state.automation_field_label(field_idx)
            };
            let mut text = format!(
                "{}Field: {}\n\n{}\n\nValue: {}",
                override_prefix,
                raw_label,
                state.config_field_help(source, field_idx),
                setting_value(state, source, field_idx),
            );
            if state.config_tab_index == 1 {
                text = format!("{}\n\n{}", coordinator_basics_text(state), text);
            }
            if source == 1 {
                if let Some(validation) = state.current_automation_field_validation() {
                    text.push_str(&format!("\n\nValidation: {}", validation));
                }
            }
            text
        } else {
            "No field selected.".to_string()
        };
        wrapped_paragraph(detail_text, "Inspector")
    };
    f.render_widget(detail_para, body_chunks[1]);
}

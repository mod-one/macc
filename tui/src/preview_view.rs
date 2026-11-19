use super::*;
pub(super) fn draw(f: &mut Frame, state: &AppState, body_area: Rect) {
    if state.preview_job.is_some() {
        f.render_widget(
            wrapped_paragraph(
                "Preparing catalog sources and comparing managed files…",
                "Preview",
            ),
            body_area,
        );
        return;
    }
    let mut preview_constraints = Vec::new();
    if state.preview_error.is_some() {
        preview_constraints.push(Constraint::Length(3));
    }
    preview_constraints.push(Constraint::Length(4));
    preview_constraints.push(Constraint::Min(0));

    let preview_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(&preview_constraints)
        .split(body_area);

    let mut chunk_index = 0;
    if let Some(error) = &state.preview_error {
        let error_para = Paragraph::new(error.clone())
            .style(Style::default().fg(crate::ui::theme().bad))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Preview Error"),
            );
        f.render_widget(error_para, preview_chunks[chunk_index]);
        chunk_index += 1;
    }

    let summary_rect = preview_chunks[chunk_index];
    chunk_index += 1;

    let mut kind_counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut project_ops = 0;
    let mut user_ops = 0;
    for op in &state.preview_ops {
        let kind = kind_label(op.kind);
        *kind_counts.entry(kind).or_insert(0) += 1;
        match op.scope {
            Scope::Project => project_ops += 1,
            Scope::User => user_ops += 1,
        }
    }

    let kind_summary = if kind_counts.is_empty() {
        "(none)".to_string()
    } else {
        kind_counts
            .iter()
            .map(|(kind, count)| format!("{} {}", kind, count))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let mut summary_text = format!(
        "Planned operations: {}\nKinds: {}\nScopes: project {} | user {}",
        state.preview_ops.len(),
        kind_summary,
        project_ops,
        user_ops
    );
    summary_text.push_str("\nReview Apply to deploy the saved configuration.");

    let summary_para = Paragraph::new(summary_text).block(panel("Summary"));
    f.render_widget(summary_para, summary_rect);

    let content_rect = preview_chunks[chunk_index];
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(content_rect);

    let mut list_state = ListState::default();
    if !state.preview_ops.is_empty() {
        let idx = state
            .preview_selection_index
            .min(state.preview_ops.len() - 1);
        list_state.select(Some(idx));
    }

    let list_items = if state.preview_ops.is_empty() {
        vec![ListItem::new(
            "No planned operations yet. Press 'r' to recompute.",
        )]
    } else {
        state
            .preview_ops
            .iter()
            .map(|op| {
                let line = format!(
                    "{:<7} {:<8} {}",
                    kind_label(op.kind),
                    scope_label(op.scope),
                    op.path
                );
                ListItem::new(line)
            })
            .collect()
    };

    let operations_list = List::new(list_items)
        .block(ui::focus_panel(
            "Planned Operations",
            navigation::Mode::Configure,
        ))
        .highlight_symbol("› ")
        .highlight_style(Style::default().bg(theme().configure_selection));
    f.render_stateful_widget(operations_list, columns[0], &mut list_state);

    let detail_text = if let Some(op) = state.selected_preview_op() {
        let mut text = format!("Path: {}\n", op.path);
        text.push_str(&format!("Action: {}\n", kind_label(op.kind)));
        text.push_str(&format!("Scope: {}\n", scope_label(op.scope)));
        text.push_str(&format!(
            "Backup required: {}\n",
            if op.metadata.backup_required {
                "yes"
            } else {
                "no"
            }
        ));
        text.push_str(&format!(
            "Consent required: {}\n",
            if op.consent_required { "yes" } else { "no" }
        ));
        text.push_str(&format!(
            "Before data: {}\n",
            if op.before.is_some() {
                "available"
            } else {
                "empty"
            }
        ));
        text.push_str(&format!(
            "After data: {}\n",
            if op.after.is_some() {
                "available"
            } else {
                "empty"
            }
        ));
        text
    } else {
        "Select an operation to see metadata.".to_string()
    };

    let detail_column = columns[1];
    let diff_view = state.preview_diff_for_selected();
    let diff_truncated = diff_view.map(|view| view.truncated).unwrap_or(false);

    let mut detail_constraints = vec![Constraint::Length(10)];
    if diff_truncated {
        detail_constraints.push(Constraint::Length(2));
    }
    detail_constraints.push(Constraint::Min(0));

    let detail_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(&detail_constraints)
        .split(detail_column);

    let metadata_rect = detail_chunks[0];
    let mut detail_index = 1;
    let trunc_rect = if diff_truncated {
        let rect = detail_chunks[detail_index];
        detail_index += 1;
        Some(rect)
    } else {
        None
    };
    let diff_rect = detail_chunks[detail_index];

    let detail_para = Paragraph::new(detail_text).block(panel("Details"));
    f.render_widget(detail_para, metadata_rect);

    if let Some(rect) = trunc_rect {
        let notice =
            Paragraph::new("Large diff truncated; scroll with PageUp/PageDown to view more.")
                .style(Style::default().fg(crate::ui::theme().warn))
                .block(panel("Truncation Notice"));
        f.render_widget(notice, rect);
    }

    let diff_text = if let Some(view) = diff_view {
        let start = state.preview_diff_scroll_position();
        let window_height = diff_rect.height as usize;
        let window_rows = window_height.saturating_sub(2).max(1);
        let line_count = view.diff.lines().count();
        let slice = view
            .diff
            .lines()
            .skip(start)
            .take(window_rows)
            .collect::<Vec<_>>();

        if slice.is_empty() {
            if line_count == 0 {
                "Diff unavailable for this operation.".to_string()
            } else if start >= line_count {
                "(End of diff)".to_string()
            } else {
                "".to_string()
            }
        } else {
            slice.join("\n")
        }
    } else {
        "Select an operation to view its diff.".to_string()
    };

    let diff_para = Paragraph::new(diff_text)
        .block(panel("Diff (PgUp/PgDn to scroll)"))
        .wrap(Wrap { trim: false });
    f.render_widget(diff_para, diff_rect);
}

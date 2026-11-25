use super::*;
use macc_core::onboarding::{ReadinessLadder, ReadinessStatus, ReadinessStep};

pub(super) fn next_step(ladder: &ReadinessLadder) -> Option<usize> {
    ladder
        .steps
        .iter()
        .position(|step| step.status == ReadinessStatus::Pending)
}

pub(super) fn status(step: &ReadinessStep, next: bool) -> (&'static str, &'static str) {
    match step.status {
        ReadinessStatus::Done => ("✓", "Verified"),
        ReadinessStatus::Pending if next => ("▲", "Action needed"),
        ReadinessStatus::Pending => ("·", "Pending"),
        ReadinessStatus::NotApplicable => ("—", "Not applicable"),
    }
}

pub(super) fn draw(f: &mut Frame, state: &AppState, body_area: Rect) {
    let snapshot = state.observations.latest.as_ref();
    let show_context = state.project_paths.is_some() && body_area.height >= 18;
    let parts = Layout::default()
        .constraints([
            Constraint::Length(1),
            Constraint::Length(u16::from(show_context)),
            Constraint::Min(1),
        ])
        .split(body_area);
    workspace_ui::tab_line(
        f,
        parts[0],
        &["Readiness", "Activity"],
        usize::from(state.home_activity),
        theme().accent,
    );
    if let Some(paths) = state.project_paths.as_ref().filter(|_| show_context) {
        let mut context = vec![paths.root.display().to_string()];
        if let Some(worktrees) = &state.worktree_status {
            if let Some(current) = &worktrees.current {
                if let Some(branch) = current.branch.as_deref() {
                    let name = branch.strip_prefix("refs/heads/").unwrap_or(branch);
                    let head = current.head.as_deref().and_then(|id| id.get(..7));
                    context
                        .push(head.map_or_else(|| name.to_owned(), |id| format!("{name} @ {id}")));
                }
            }
            if worktrees.error.is_none() {
                context.push(format!("{} worktrees", worktrees.total));
            }
        }
        f.render_widget(
            Paragraph::new(context.join(" · ")).style(Style::default().fg(theme().muted)),
            parts[1],
        );
    }
    let (overview_area, readiness_area) = workspace_ui::columns(parts[2]);
    let body_chunks = [overview_area, readiness_area];

    if let Some(obs) =
        snapshot.filter(|obs| !state.home_activity && !obs.readiness.steps.is_empty())
    {
        draw_readiness(f, state, obs, overview_area, readiness_area);
        return;
    }
    let mut overview = format!(
        "{} pending decisions · {} pending approvals\n",
        state.decision_center.pending(),
        state.awaiting_approvals().count()
    );
    if let Some(run) = &state.coordinator_run_summary {
        overview.push_str(&format!(
            "Last run: {}\n{}\n",
            run.headline, run.next_action
        ));
        if let Some((id, note)) = run
            .previous_diagnosis()
            .filter(|_| run.supervisor.is_none())
        {
            overview.push_str(&format!(
                "Previous incident ({id}) · {}\n{}\n",
                note.heading(),
                note.diagnosis
            ));
            if let Some(result) = &note.outcome {
                overview.push_str(&format!("{result}\n"));
            }
        }
    }
    overview.push('\n');
    if !state.errors.is_empty() {
        overview.push_str("Errors:\n");
        for err in &state.errors {
            overview.push_str(&format!("- {}\n", err));
        }
        overview.push('\n');
    }
    if !state.notices.is_empty() {
        overview.push_str("Notices:\n");
        for notice in &state.notices {
            overview.push_str(&format!("- {}\n", notice));
        }
        overview.push('\n');
    }
    if let Some(paths) = &state.project_paths {
        overview.push_str(&format!("Project Root: {}\n", paths.root.display()));
    }
    if let Some(config) = &state.working_copy {
        let titles: Vec<String> = config
            .tools
            .enabled
            .iter()
            .map(|id| {
                state
                    .tool_descriptors
                    .iter()
                    .find(|d| &d.id == id)
                    .map(|d| d.title.clone())
                    .unwrap_or_else(|| id.clone())
            })
            .collect();
        overview.push_str(&format!("Enabled Tools: {}\n", titles.join(", ")));
        let mcp_selected = config
            .selections
            .as_ref()
            .map(|s| s.mcp.clone())
            .unwrap_or_default();
        overview.push_str(&format!(
            "MCP Servers Selected: {}\n",
            if mcp_selected.is_empty() {
                "(none)".to_string()
            } else {
                mcp_selected.join(", ")
            }
        ));
    } else if state.errors.is_empty() {
        overview.push_str("No configuration loaded.\n");
    }

    if let Some(status) = &state.worktree_status {
        if let Some(err) = &status.error {
            overview.push_str(&format!("Worktree Status: unavailable ({})\n", err));
        } else if let Some(current) = &status.current {
            let branch = current.branch.as_deref().unwrap_or("-");
            let head = current.head.as_deref().unwrap_or("-");
            let name = current
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("-");
            overview.push_str(&format!(
                "Worktree: {} (branch: {}, head: {})\n",
                name, branch, head
            ));
            overview.push_str(&format!(
                "Worktrees Total (includes retained/support slots): {}\n{}\n",
                status.total, status.activity
            ));
        } else {
            overview.push_str(&format!(
                "Worktree: (none) | Worktrees Total: {}\n",
                status.total
            ));
        }
    }

    if let Some(run) = state
        .coordinator_run_summary
        .as_ref()
        .filter(|_| state.home_activity)
    {
        for past in run.recent_runs.iter().take(10) {
            overview.push_str(&format!(
                "\n{}  {}  {}",
                relative_time_label(&past.started_at),
                past.status,
                past.stop_reason.as_deref().unwrap_or("")
            ));
        }
    }
    let overview_para = wrapped_paragraph(
        overview,
        if state.home_activity {
            "Activity"
        } else {
            "Overview"
        },
    )
    .scroll((state.inspector_scroll, 0));
    f.render_widget(overview_para, body_chunks[0]);

    // Readiness ladder (spec §9 / §13.1) — via Engine facade.
    let readiness_text = if let Some(obs) = snapshot {
        build_readiness_text(&obs.readiness, state.home_doctor_summary.as_deref())
    } else if state.project_paths.is_some() {
        "Checking project readiness…".into()
    } else {
        "Run 'macc init' to set up this project.\n\nActions:\n  [d] Doctor check\n  CLI: macc quickstart".to_string()
    };
    let steps_para = wrapped_paragraph(readiness_text, "Readiness");
    f.render_widget(steps_para, body_chunks[1]);
}

fn draw_readiness(
    f: &mut Frame,
    state: &AppState,
    obs: &observations::Snapshot,
    list: Rect,
    inspector: Rect,
) {
    let theme = theme();
    let steps = &obs.readiness.steps;
    let actionable = next_step(&obs.readiness);
    let selected = state.inspector_index.min(steps.len() - 1);
    let requests = workbench::pending(state);
    let mut attention = Vec::new();
    if requests > 0 {
        attention.push(Line::styled(
            format!("▲ {requests} pending   [I] Inbox"),
            Style::default().fg(theme.warn),
        ));
    }
    if let Some(intervention) = obs
        .intervention
        .as_ref()
        .filter(|i| i.status != "recovered")
    {
        attention.push(Line::styled(
            format!("▲ Supervisor {}   [C] Coordinator", intervention.status),
            Style::default().fg(theme.warn),
        ));
    }
    if workbench::has_draft(state) {
        attention.push(Line::styled(
            "◆ Unsaved edits   [Ctrl+T] Configure",
            Style::default().fg(theme.draft),
        ));
    }
    let list = if attention.is_empty() || list.height < 14 {
        list
    } else {
        let split = Layout::default()
            .constraints([
                Constraint::Min(5),
                Constraint::Length(attention.len() as u16 + 2),
            ])
            .split(list);
        f.render_widget(
            Paragraph::new(attention).block(panel("Needs you")),
            split[1],
        );
        split[0]
    };
    let done = steps
        .iter()
        .filter(|step| step.status == ReadinessStatus::Done)
        .count();
    let title = format!("Readiness · {done} of {}", steps.len());
    let mut selection = ListState::default().with_selected(Some(selected));
    let rows = steps.iter().enumerate().map(|(index, step)| {
        let (glyph, _) = status(step, Some(index) == actionable);
        let mut parts = vec![
            Span::styled(
                format!("{}. ", step.number),
                Style::default().fg(theme.muted),
            ),
            Span::styled(
                glyph,
                Style::default().fg(theme.glyph_color(glyph).unwrap_or(theme.foreground)),
            ),
            Span::raw(format!("  {}", step.label)),
        ];
        if let Some(detail) = &step.detail {
            parts.push(Span::styled(
                format!("   {detail}"),
                Style::default().fg(theme.muted),
            ));
        }
        ListItem::new(Line::from(parts))
    });
    f.render_stateful_widget(
        List::new(rows)
            .block(ui::focus_panel(&title, navigation::Mode::Operate))
            .highlight_symbol("› ")
            .highlight_style(
                Style::default()
                    .bg(theme.operate_selection)
                    .add_modifier(Modifier::BOLD),
            ),
        list,
        &mut selection,
    );
    let step = &steps[selected];
    let (glyph, label) = status(step, Some(selected) == actionable);
    let mut lines = vec![
        Line::styled(&*step.label, Style::default().add_modifier(Modifier::BOLD)),
        Line::styled(
            format!("{glyph} {label} · step {} of {}", step.number, steps.len()),
            Style::default().fg(theme.glyph_color(glyph).unwrap_or(theme.foreground)),
        ),
    ];
    if let Some(detail) = &step.detail {
        lines.push(Line::default());
        lines.push(Line::raw(detail.as_str()));
    }
    if let Some(fix) =
        readiness_step_fix(step.number).filter(|_| step.status == ReadinessStatus::Pending)
    {
        lines.push(Line::default());
        lines.push(Line::styled(
            "NEXT ACTION",
            Style::default()
                .fg(theme.muted)
                .add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::raw(fix.action));
    }
    f.render_widget(
        Paragraph::new(lines)
            .block(panel("Inspector"))
            .wrap(Wrap { trim: false })
            .scroll((state.inspector_scroll, 0)),
        inspector,
    );
}

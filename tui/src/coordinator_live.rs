use super::*;
mod live_table;
pub(super) fn draw(f: &mut Frame, state: &AppState, body_area: Rect) {
    let theme = theme();
    let snapshot = state.observations.latest.as_ref();
    let durable_run_active = state
        .coordinator_run_summary
        .as_ref()
        .is_some_and(|summary| matches!(summary.status.as_str(), "running" | "draining"));
    let coordinator_active = state.is_coordinator_running() || durable_run_active;
    let (marker, color, status) = if state.is_coordinator_paused() {
        ("▲", theme.warn, "Paused".to_owned())
    } else if coordinator_active {
        let command = state
            .coordinator_running_command
            .as_deref()
            .unwrap_or("Running");
        (
            "●",
            theme.good,
            format!(
                "{command} ({}) {}",
                format_hms(state.coordinator_elapsed_seconds().unwrap_or(0)),
                state.coordinator_spinner_frame()
            ),
        )
    } else {
        ("○", theme.muted, "Idle".to_owned())
    };
    let project = Line::from(vec![
        Span::styled(format!("{marker} {status}  "), Style::default().fg(color)),
        Span::styled(
            format!(
                "PRD {}  ",
                snapshot.map_or("Reading queue…", |o| o.prd.as_str())
            ),
            Style::default().fg(theme.muted),
        ),
    ]);
    let mut summary = Vec::new();
    if let Some(tasks) = &state.coordinator_snapshot {
        summary.extend([
            Span::raw(format!("tasks {} · todo {}  ", tasks.total, tasks.todo)),
            Span::styled(
                format!("● active {}  ", tasks.active),
                Style::default().fg(theme.good),
            ),
            Span::styled(
                format!("✕ blocked {}  ", tasks.blocked),
                Style::default().fg(if tasks.blocked > 0 {
                    theme.bad
                } else {
                    theme.muted
                }),
            ),
            Span::styled(
                format!("✓ merged {}", tasks.merged),
                Style::default().fg(theme.good),
            ),
        ]);
    } else {
        summary.push(Span::raw("Tasks unavailable"));
    }
    if !state.search_query.is_empty() {
        summary.push(Span::raw(format!("  Search: {}", state.search_query)));
    }
    let counts = Line::from(summary);
    let mut lines = if project.width() + counts.width() > usize::from(body_area.width) {
        vec![project, counts]
    } else {
        let mut combined = project;
        combined.spans.extend(counts.spans);
        vec![combined]
    };
    if !state.coordinator_throttled_tools.is_empty() {
        lines.push(Line::styled(
            tool_cooldowns::label(&state.coordinator_throttled_tools),
            Style::default().fg(theme.warn),
        ));
    }
    let live_chunks = Layout::vertical([
        Constraint::Length(if body_area.width < 65 { 2 } else { 1 }),
        Constraint::Length(lines.len() as u16),
        Constraint::Min(0),
    ])
    .split(body_area);
    let header = if live_chunks[0].height > 1 {
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(live_chunks[0])
    } else {
        Layout::horizontal([
            Constraint::Min(24),
            Constraint::Length(body_area.width.saturating_sub(24).min(72)),
        ])
        .split(live_chunks[0])
    };
    workspace_ui::tab_line(f, header[0], &["Live", "Workers"], 0, theme.accent);
    render_coordinator_ownership_banner(f, header[1], state);
    f.render_widget(Paragraph::new(lines), live_chunks[1]);

    let body_chunks = workbench::live_panes(live_chunks[2], state.coordinator_log_pane_visible);

    // Pane 1: LIVE TASKS Table
    let filtered_tasks = state.filtered_active_tasks();
    let snapshot_total = state
        .coordinator_snapshot
        .as_ref()
        .map(|s| s.total)
        .unwrap_or(0);
    let coordinator_live_empty = !state.is_coordinator_running()
        && !state.is_coordinator_paused()
        && snapshot_total == 0
        && filtered_tasks.is_empty();
    // The supervisor is not a registry task: it gets its own pinned,
    // differently styled row so it cannot be mistaken for one.
    let supervisor_row = snapshot.and_then(|o| o.intervention.as_ref());
    let supervisor_now = chrono::Utc::now();
    let selected_row = state.live_row_index(filtered_tasks.len());
    live_table::draw(
        f,
        body_chunks[0],
        &filtered_tasks,
        supervisor_row,
        supervisor_now,
        selected_row,
        || {
            coordinator_live_empty_lines(
                coordinator_live_empty,
                !state.search_query.is_empty(),
                snapshot_total,
            )
        },
    );
    let selected_task = selected_row
        .and_then(|row| row.checked_sub(usize::from(supervisor_row.is_some())))
        .and_then(|index| filtered_tasks.get(index));
    let mut detail_lines = Vec::new();
    if let Some(t) = selected_task {
        let full_task = state
            .coordinator_registry_for_render()
            .and_then(|r| r.tasks.iter().find(|rt| rt.id == t.task_id).cloned());
        let title = full_task
            .as_ref()
            .and_then(|rt| rt.title.clone())
            .unwrap_or_else(|| "(no title)".to_string());
        let worktree = full_task
            .as_ref()
            .and_then(|rt| rt.task_runtime.worktree.clone())
            .unwrap_or_else(|| {
                t.worktree
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "-".to_string())
            });
        let branch = full_task
            .as_ref()
            .and_then(|rt| rt.task_runtime.branch.clone())
            .unwrap_or_else(|| t.branch.clone().unwrap_or_else(|| "-".to_string()));

        detail_lines.push(Line::from(vec![
            Span::styled("Task ID:    ", Style::default().fg(theme.muted)),
            Span::styled(&t.task_id, Style::default().add_modifier(Modifier::BOLD)),
            Span::styled("   Title: ", Style::default().fg(theme.muted)),
            Span::styled(title, Style::default()),
        ]));
        detail_lines.push(Line::from(vec![
            Span::styled("Worktree:   ", Style::default().fg(theme.muted)),
            Span::styled(worktree, Style::default()),
            Span::styled("   Branch: ", Style::default().fg(theme.muted)),
            Span::styled(branch, Style::default()),
        ]));
        detail_lines.push(Line::from(vec![
            Span::styled("Status:     ", Style::default().fg(theme.muted)),
            Span::styled(
                format!(
                    "{} (phase: {})",
                    t.runtime_status.as_str(),
                    t.phase.compact_label()
                ),
                Style::default(),
            ),
            Span::styled("   Tool: ", Style::default().fg(theme.muted)),
            Span::styled(&t.tool, Style::default()),
        ]));
        if let Some(gate) = full_task.as_ref().and_then(|task| task.gate.as_ref()) {
            let verdict = full_task
                .as_ref()
                .and_then(|task| task.task_runtime.gate_verdict)
                .map(|value| format!("{value:?}").to_ascii_lowercase())
                .unwrap_or_else(|| "pending".to_string());
            detail_lines.push(Line::from(vec![
                Span::styled("Gate:       ", Style::default().fg(theme.muted)),
                Span::styled(
                    format!("{} (required: {:?})", verdict, gate.required_verdict)
                        .to_ascii_lowercase(),
                    if verdict == "accepted" {
                        Style::default().fg(theme.good)
                    } else {
                        Style::default().fg(theme.warn)
                    },
                ),
            ]));
        }
        if let Some(block) = full_task
            .as_ref()
            .and_then(|task| task.blocked_on_external.as_ref())
        {
            detail_lines.push(Line::from(vec![
                Span::styled("External:   ", Style::default().fg(theme.muted)),
                Span::styled(
                    format!(
                        "{}; clears when {} (source: {:?})",
                        block.reason, block.clears_when, block.source
                    )
                    .to_ascii_lowercase(),
                    Style::default().fg(theme.warn),
                ),
            ]));
        }
        if let Some(ref msg) = t.current_message {
            detail_lines.push(Line::from(vec![
                Span::styled("Message:    ", Style::default().fg(theme.muted)),
                Span::styled(msg, Style::default()),
            ]));
        }
        if let Some(ref err) = t.last_error {
            detail_lines.push(Line::from(vec![
                Span::styled("Last Error: ", Style::default().fg(theme.muted)),
                Span::styled(err, Style::default().fg(theme.bad)),
            ]));
        }
        if let Some(ref exp) = t.result_explanation {
            detail_lines.push(Line::from(vec![
                Span::styled("Explanation:", Style::default().fg(theme.muted)),
                Span::styled(format!(" {}", exp), Style::default().fg(theme.bad)),
            ]));
        }
    } else if let Some(live) = supervisor_row {
        for (label, text, kind) in supervisor::detail(live, supervisor_now) {
            let style = match kind {
                supervisor::DetailKind::Problem => Style::default().fg(theme.bad),
                supervisor::DetailKind::Action => Style::default().fg(theme.accent),
                supervisor::DetailKind::Info => Style::default(),
            };
            detail_lines.push(Line::from(vec![
                Span::styled(label, Style::default().fg(theme.muted)),
                Span::styled(text, style),
            ]));
        }
    } else {
        if coordinator_live_empty {
            detail_lines.push(Line::from(
                "Select a task to inspect its worktree, activity and delivery evidence.",
            ));
        } else if !state.search_query.is_empty() {
            detail_lines.push(Line::from(
                "No task matches the current filter. Edit the search to show other tasks.",
            ));
        } else {
            detail_lines.push(Line::from(
                "No task selected. When tasks appear, use ↑/↓ to navigate.",
            ));
        }
    }
    let detail_title = if selected_task.is_none() && supervisor_row.is_some() {
        "SUPERVISOR TASK DETAIL"
    } else {
        "SELECTED TASK DETAIL"
    };
    let detail_para = Paragraph::new(detail_lines)
        .block(panel(detail_title))
        .wrap(Wrap { trim: true });
    f.render_widget(detail_para, body_chunks[1]);

    // Pane 3: live signal while running, durable run result while idle.
    if state.coordinator_log_pane_visible {
        let mut logs_lines = Vec::new();
        let panel_title;
        if !coordinator_active {
            panel_title = "LAST RESULT";
            if let Some(summary) = state.coordinator_run_summary.as_ref() {
                let severity_style = match summary.severity.as_str() {
                    "error" => Style::default().fg(theme.bad).add_modifier(Modifier::BOLD),
                    "warning" => Style::default().fg(theme.warn).add_modifier(Modifier::BOLD),
                    "success" => Style::default().fg(theme.good).add_modifier(Modifier::BOLD),
                    _ => Style::default().fg(theme.accent),
                };
                logs_lines.push(Line::from(vec![
                    Span::styled(
                        format!(
                            "{} {}",
                            match summary.severity.as_str() {
                                "error" => "✕",
                                "warning" => "▲",
                                "success" => "✓",
                                _ => "ℹ",
                            },
                            summary.severity.to_ascii_uppercase()
                        ),
                        severity_style,
                    ),
                    Span::raw("  "),
                    Span::styled(
                        summary.status.clone(),
                        Style::default().fg(theme.accent_dim),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        relative_time_label(&summary.occurred_at),
                        Style::default().fg(theme.muted),
                    ),
                ]));
                logs_lines.push(Line::from(Span::styled(
                    summary.headline.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                )));
                if summary.unmet_preconditions.is_empty() {
                    logs_lines.push(Line::from(vec![
                        Span::styled("Cause: ", Style::default().fg(theme.muted)),
                        Span::raw(summary.cause.clone()),
                    ]));
                } else {
                    logs_lines.push(Line::from(vec![
                    Span::styled("Cause: ", Style::default().fg(theme.muted)),
                    Span::raw(format!(
                        "{} cannot be implemented because the following precondition{} not satisfied:",
                        summary.task_id.as_deref().unwrap_or("the blocked task"),
                        if summary.unmet_preconditions.len() == 1 { " is" } else { "s are" }
                    )),
                ]));
                    for item in &summary.unmet_preconditions {
                        logs_lines.push(Line::from(format!("  - {}", item)));
                    }
                }
                if !summary.out_of_scope_paths.is_empty() {
                    logs_lines.push(Line::from(vec![
                        Span::styled("Out of scope: ", Style::default().fg(theme.warn)),
                        Span::raw(format!(
                            "the fix lies in {} (another module): a repair is needed, not a retry",
                            summary.out_of_scope_paths.join(", ")
                        )),
                    ]));
                }
                if !summary.dependent_task_ids.is_empty() {
                    logs_lines.push(Line::from(vec![
                        Span::styled("Affected: ", Style::default().fg(theme.muted)),
                        Span::raw(summary.dependent_task_ids.join(", ")),
                    ]));
                }
                logs_lines.push(Line::from(vec![
                    Span::styled("Next: ", Style::default().fg(theme.accent)),
                    Span::raw(summary.next_action.clone()),
                ]));
                if summary.repeated_count > 1 {
                    logs_lines.push(Line::from(Span::styled(
                    format!(
                        "Same result occurred {} times in the last {} runs. Do not retry unchanged.",
                        summary.repeated_count,
                        summary.recent_runs.len()
                    ),
                    Style::default().fg(theme.warn),
                )));
                }
                if summary.recent_runs.len() > 1 {
                    logs_lines.push(Line::from(Span::styled(
                        "Recent runs:",
                        Style::default().fg(theme.muted),
                    )));
                    for run in summary.recent_runs.iter().take(5) {
                        let at = run.stopped_at.as_deref().unwrap_or(&run.started_at);
                        logs_lines.push(Line::from(format!(
                            "  {}  {}  {}",
                            relative_time_label(at),
                            run.status,
                            ui::truncate_middle(
                                run.stop_reason.as_deref().unwrap_or("no reason recorded"),
                                120,
                            )
                        )));
                    }
                }
            } else {
                logs_lines.push(Line::from("No coordinator run has been recorded yet."));
                logs_lines.push(Line::from(
                    "Next: prepare a PRD task, then start the coordinator.",
                ));
            }
        } else if let Some(t) = selected_task {
            panel_title = "LIVE LOGS";
            let full_task = state
                .coordinator_registry_for_render()
                .and_then(|r| r.tasks.iter().find(|rt| rt.id == t.task_id).cloned());
            let mut read_success = false;
            if let Some(ref ft) = full_task {
                if let Some(ref stdout_rel) = ft.task_runtime.stdout_log {
                    if let Some(ref paths) = state.project_paths {
                        let path = paths.root.join(stdout_rel);
                        if path.exists() {
                            let lines = get_last_lines_of_file(&path, 15);
                            if !lines.is_empty() {
                                logs_lines.push(Line::from(vec![Span::styled(
                                    format!("Source: stdout ({})", stdout_rel),
                                    Style::default().fg(theme.accent_dim),
                                )]));
                                for l in lines {
                                    logs_lines.push(Line::from(l));
                                }
                                read_success = true;
                            }
                        }
                    }
                }
            }
            if !read_success {
                logs_lines.push(Line::from(
                    "No active stdout log file found. Showing matching coordinator events:",
                ));
                for line in state.coordinator_events.iter().rev() {
                    if line.contains(&t.task_id) {
                        logs_lines.push(Line::from(line.clone()));
                    }
                }
            }
        } else {
            panel_title = "LIVE LOGS";
            if coordinator_live_empty {
                logs_lines.extend(coordinator_live_empty_lines(true, false, snapshot_total));
            } else {
                logs_lines.push(Line::from(
                    "No task selected. Showing recent coordinator events:",
                ));
            }
            for line in state.coordinator_events.iter().rev().take(15).rev() {
                logs_lines.push(Line::from(line.clone()));
            }
        }
        // Human approvals come first, live or idle: the run is alive but
        // cannot proceed until someone decides, and that is the one
        // thing the operator must see.
        let approval_lines = approval_request_lines(&state.coordinator_pending_approvals);
        let panel_title = if approval_lines.is_empty() {
            panel_title
        } else {
            "APPROVAL REQUIRED"
        };
        let logs_lines = if approval_lines.is_empty() {
            logs_lines
        } else {
            let mut all = approval_lines
                .into_iter()
                .map(|(text, strong)| {
                    if strong {
                        Line::from(Span::styled(
                            text,
                            Style::default().fg(theme.warn).add_modifier(Modifier::BOLD),
                        ))
                    } else {
                        Line::from(text)
                    }
                })
                .collect::<Vec<_>>();
            all.push(Line::from(""));
            all.extend(logs_lines);
            all
        };
        let logs_para = Paragraph::new(logs_lines)
            .block(panel(panel_title))
            .wrap(Wrap { trim: true });
        f.render_widget(logs_para, body_chunks[2]);
    }

    // Render popups if open
    if let Some(ref diff) = state.coordinator_task_diff_popup {
        let area = ui::centered_rect(85, 85, ui::content_area(f.size()));
        let diff_para = Paragraph::new(diff.as_str())
            .block(panel("Task diff"))
            .scroll((state.inspector_scroll, 0))
            .wrap(Wrap { trim: false });
        f.render_widget(Clear, area);
        f.render_widget(diff_para, area);
    }
    if let Some(ref explain) = state.coordinator_task_explain_popup {
        let area = ui::centered_rect(85, 85, ui::content_area(f.size()));
        let explain_para = Paragraph::new(explain.as_str())
            .block(panel("Recorded evidence"))
            .scroll((state.inspector_scroll, 0))
            .wrap(Wrap { trim: false });
        f.render_widget(Clear, area);
        f.render_widget(explain_para, area);
    }
}

#[cfg(test)]
mod tests;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use macc_core::service::coordinator_workflow::CoordinatorCommand;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState,
        Wrap,
    },
    Frame, Terminal,
};
use std::{collections::BTreeMap, io, time::Duration};

pub mod approval;
mod automation_view;
mod command_catalog;
mod command_footer;
mod command_palette;
#[cfg(test)]
mod command_tests;
mod coordinator_live;
mod coordinator_poll;
pub mod decision_center;
mod doctor_view;
mod inbox;
mod navigation;
mod observations;
mod overview_view;
mod preview;
mod preview_view;
mod tools_view;
mod workbench;
use command_palette::*;
mod focus;
use focus::widget_input;
mod hooks;
#[cfg(test)]
mod hooks_tests;
mod o2;
pub mod ownership;
pub mod prd_queue;
pub mod screen;
mod session_checkpoints;
mod session_navigation;
mod session_usage;
pub mod sessions;
#[cfg(test)]
mod sessions_tests;
pub mod state;
mod supervisor;
mod tool_cooldowns;
pub mod ui;
#[cfg(test)]
mod workspace_tests;
mod workspace_ui;

use macc_core::plan::{PlannedOpKind, Scope};
use macc_core::tool::{FieldDefault, FieldKind};
use screen::Screen;
use state::{AppState, UiStatusLevel};
use ui::{header_lines, panel, theme, wrapped_paragraph, HeaderContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMode {
    Default,
    /// Launch directly into the coordinator live screen.
    /// `phase_overrides` is a human-readable summary of active runtime phase overrides
    /// (e.g. `"[testing:off] [review:required]"`), or `None` when none are active.
    CoordinatorRun {
        phase_overrides: Option<String>,
    },
    /// Attach to an already running and supervised coordinator.
    CoordinatorAttach {
        phase_overrides: Option<String>,
    },
    /// Launch into the read-only observer/watch screen (`macc status --watch`).
    /// `control` enables operator actions (kill, stop, retry).
    /// `logs_only` collapses all panes except the log tail.
    /// `events_only` collapses all panes except the event timeline.
    Watch {
        control: bool,
        logs_only: bool,
        events_only: bool,
    },
}

/// RAII guard to ensure terminal state is restored on drop.
struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen,);
        let _ = self.terminal.show_cursor();
    }
}

pub fn run_tui() -> Result<()> {
    run_tui_with_launch(LaunchMode::Default)
}

pub fn run_tui_with_launch(mode: LaunchMode) -> Result<()> {
    let mut guard = TerminalGuard::new()?;
    let registry = macc_registry::default_registry();
    let engine = std::sync::Arc::new(macc_core::MaccEngine::new(registry));
    let mut state = AppState::new(engine);
    if let Some(paths) = &state.project_paths {
        macc_core::catalog::availability::schedule(paths);
    }
    match mode {
        LaunchMode::CoordinatorRun { phase_overrides } => {
            state.goto_screen(Screen::CoordinatorLive);
            state.start_coordinator_command(CoordinatorCommand::Run);
            state.coordinator_run_auto_quit = true;
            state.coordinator_phase_overrides = phase_overrides;
        }
        LaunchMode::CoordinatorAttach { phase_overrides } => {
            state.goto_screen(Screen::CoordinatorLive);
            state.coordinator_phase_overrides = phase_overrides;
            state.coordinator_run_auto_quit = false;
        }
        LaunchMode::Watch {
            control,
            logs_only,
            events_only,
        } => {
            state.goto_screen(Screen::Watch);
            state.watch_control_enabled = control;
            state.watch_logs_only = logs_only;
            state.watch_events_only = events_only;
        }
        LaunchMode::Default => {}
    }

    run_app(&mut guard.terminal, &mut state)?;

    Ok(())
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, state: &mut AppState) -> io::Result<()> {
    let mut catalog_tick = std::time::Instant::now() - Duration::from_secs(2);
    loop {
        if state.current_screen() == Screen::Skills
            && catalog_tick.elapsed() >= Duration::from_secs(2)
        {
            if let Some(paths) = &state.project_paths {
                macc_core::catalog::availability::schedule(paths);
            }
            state.refresh_skills();
            catalog_tick = std::time::Instant::now();
        }
        state.tick();
        terminal.draw(|f| ui(f, state, true))?;

        if event::poll(Duration::from_millis(150))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if key
                        .modifiers
                        .intersects(KeyModifiers::ALT | KeyModifiers::SUPER)
                    {
                        continue;
                    }
                    handle_shortcut(
                        state,
                        key.code,
                        key.modifiers.contains(KeyModifiers::CONTROL),
                    );
                }
            }
        }
        if state.should_quit {
            state.release_ownership_on_exit();
            return Ok(());
        }
    }
}

fn format_hms(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    format!("{}:{:02}:{:02}", hours, minutes, seconds)
}

fn relative_time_label(timestamp: &str) -> String {
    let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(timestamp) else {
        return timestamp.to_string();
    };
    let seconds = chrono::Utc::now()
        .signed_duration_since(timestamp.with_timezone(&chrono::Utc))
        .num_seconds()
        .max(0);
    if seconds < 60 {
        format!("{}s ago", seconds)
    } else if seconds < 3_600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3_600)
    } else {
        format!("{}d ago", seconds / 86_400)
    }
}

#[cfg(test)]
fn handle_key(state: &mut AppState, key: KeyCode) {
    handle_shortcut(state, key, false);
}

fn handle_shortcut(state: &mut AppState, key: KeyCode, control: bool) {
    if matches!(key, KeyCode::F(_)) {
        return;
    }
    if let Some(command) = command_catalog::resolve(state, key, control) {
        if command.enabled {
            command_catalog::execute(state, command.action);
        } else {
            state.set_status(
                UiStatusLevel::Warning,
                format!("{} is unavailable. {}", command.label, command.hint),
                Some(Duration::from_secs(2)),
            );
        }
    } else if control {
        return;
    } else if state.command_palette_open {
        if let KeyCode::Char(c) = key {
            state.command_palette_query.push(c);
            state.command_palette_selection = 0;
        }
    } else if !state.help_open && focus::Layer::current(state).accepts_input(state) {
        widget_input(state, key);
    }
}

fn ui(f: &mut Frame, state: &AppState, full_clear: bool) {
    draw_client(f, state, full_clear);
    // Cleared areas and modal widgets can restore the terminal default. Resolve
    // those cells against the same palette as the rest of the application.
    let palette = theme();
    for cell in &mut f.buffer_mut().content {
        if cell.fg == Color::Reset {
            cell.fg = palette.foreground;
        }
        if cell.bg == Color::Reset {
            cell.bg = palette.background;
        }
    }
}

fn draw_client(f: &mut Frame, state: &AppState, full_clear: bool) {
    if let Some(editor) = &state.prd_queue_editor {
        editor.draw_in(f, ui::content_area(f.size()));
        draw_overlays(f, state);
        return;
    }
    let theme = theme();
    if full_clear {
        f.render_widget(Clear, f.size());
    }
    let current_screen = state.current_screen();
    let frame_width = f.size().width;
    let next_step = if current_screen == Screen::Home {
        next_step_text(state, frame_width as usize)
    } else {
        String::new()
    };
    let snapshot = state.observations.latest.as_ref();
    let trust_strip = snapshot.and_then(|o| o.trust.clone());
    let supervisor_strip = snapshot
        .filter(|_| current_screen == Screen::CoordinatorLive)
        .and_then(|o| o.supervisor_status.clone());
    let override_strip = match (&state.coordinator_phase_overrides, supervisor_strip) {
        (Some(phases), Some(status)) => Some(format!("{phases} | {status}")),
        (phases, status) => status.or_else(|| phases.clone()),
    };
    let project_label = state
        .project_paths
        .as_ref()
        .map(|p| {
            p.root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("project")
                .to_string()
        })
        .unwrap_or_else(|| "(no project)".to_string());
    let config_status = if state.working_copy.is_some() {
        "ok"
    } else {
        "missing"
    };
    let draft = workbench::changes::draft(state);
    let header_ctx = HeaderContext {
        mode: navigation::mode(state),
        page: if current_screen == Screen::CoordinatorLive {
            "Coordinator › Live"
        } else {
            navigation::current(state).label()
        },
        draft_count: draft.len(),
        inbox: workbench::pending(state),
        project: &project_label,
        config_label: config_status,
        errors: state.errors.len(),
        coordinator_active: state.is_coordinator_running(),
        coordinator_paused: state.is_coordinator_paused(),
        coordinator_stopped: state.unresolved_run().is_some(),
        action_needed: state.project_paths.is_none()
            || snapshot.is_some_and(|obs| overview_view::next_step(&obs.readiness).is_some()),
        next_step: next_step.strip_prefix("Next step: ").unwrap_or(&next_step),
        status: state.status_line(),
        width: frame_width,
        trust_strip,
        override_strip,
    };
    let lines = header_lines(&header_ctx, &theme);
    let chunks = Layout::default()
        .constraints([
            Constraint::Length(lines.len() as u16),
            Constraint::Min(0),
            Constraint::Length(ui::command_height(f.size())),
        ])
        .split(f.size());
    f.render_widget(Paragraph::new(lines), chunks[0]);
    let body_area = chunks[1];
    let body_area = workbench::rail(f, state, body_area, draft.len());
    let draft_label = if draft.is_empty() {
        "Draft".into()
    } else {
        format!("Draft ◆ {}", draft.len())
    };
    let change_tabs = [draft_label.as_str(), "Preview", "Apply"];
    let tabs = match current_screen {
        Screen::Changes => Some((&change_tabs[..], 0)),
        Screen::Preview => Some((&change_tabs[..], 1)),
        Screen::Apply => Some((&change_tabs[..], 2)),
        Screen::Watch => Some((&["Live", "Workers"][..], 1)),
        _ => None,
    };
    let body_area = if let Some((labels, selected)) = tabs {
        let panes = Layout::default()
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(body_area);
        let accent = if navigation::mode(state) == navigation::Mode::Configure {
            theme.configure
        } else {
            theme.accent
        };
        workspace_ui::tab_line(f, panes[0], labels, selected, accent);
        panes[1]
    } else {
        body_area
    };
    match current_screen {
        Screen::Inbox => inbox::draw(f, state, body_area),
        Screen::Doctor => doctor_view::draw(f, state, body_area),
        Screen::Changes => workbench::changes::draw(f, state, body_area, &draft),
        Screen::Skills => {
            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(body_area);
            let selected_skills = state.selected_skills();

            let mut list_state = ListState::default();
            let visible = state.filtered_skill_indices();
            if !visible.is_empty() {
                let idx = if visible.contains(&state.skill_selection_index) {
                    state.skill_selection_index
                } else {
                    visible[0]
                };
                let selected_pos = visible.iter().position(|v| *v == idx).unwrap_or(0);
                list_state.select(Some(selected_pos));
            }

            let items: Vec<ListItem> = state
                .filtered_skill_indices()
                .iter()
                .map(|index| {
                    let skill = &state.skills[*index];
                    let is_enabled = selected_skills.contains(&skill.id.to_string());
                    let enabled_marker = if is_enabled { "[x]" } else { "[ ]" };
                    let mandatory_badge =
                        if skill.availability.as_ref().is_some_and(|a| a.disabled()) {
                            " [unavailable]"
                        } else if skill.mandatory {
                            " [mandatory]"
                        } else {
                            ""
                        };
                    ListItem::new(Line::from(vec![
                        Span::raw(enabled_marker),
                        Span::raw(" "),
                        Span::raw(format!("{}{}", skill.name, mandatory_badge)),
                    ]))
                })
                .collect();

            let title = format!(
                "Skills ({}/{}, shown {})",
                selected_skills.len(),
                state.skills.len(),
                visible.len()
            );
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(title))
                .highlight_symbol("› ")
                .highlight_style(Style::default().bg(theme.selection(navigation::mode(state))));
            f.render_stateful_widget(list, body_chunks[0], &mut list_state);

            if visible.is_empty() {
                f.render_widget(
                    Paragraph::new("No matching skills. Press '/' to edit search.")
                        .block(Block::default().borders(Borders::ALL)),
                    body_chunks[1],
                );
            } else {
                let selected = if visible.contains(&state.skill_selection_index) {
                    state.skill_selection_index
                } else {
                    visible[0]
                };
                let current_skill = &state.skills[selected];
                let mut desc_text = format!("ID: {}\n\n", current_skill.id);
                desc_text.push_str("Description:\n");
                desc_text.push_str(&current_skill.description);
                if let Some(a) = &current_skill.availability {
                    desc_text.push_str(&format!("\n\nAvailability: {:?} — {}", a.state, a.reason));
                    if a.stale {
                        desc_text.push_str("\nCached result; press [r] to recheck.");
                    }
                    if let Some(error) = &a.last_error {
                        desc_text.push_str(&format!("\nLast check: {error}"));
                    }
                } else {
                    desc_text.push_str("\n\nAvailability: not verified yet.");
                }
                if let Some(paths) = &state.project_paths {
                    if macc_core::catalog::availability::checking(paths) {
                        desc_text.push_str("\nChecking sources in the background…");
                    }
                }
                if current_skill.mandatory {
                    desc_text.push_str("\n\nMandatory skill: always enabled from catalog policy.");
                }

                let desc_para = Paragraph::new(desc_text)
                    .wrap(Wrap { trim: false })
                    .block(panel("Details"));
                f.render_widget(desc_para, body_chunks[1]);
            }
        }
        Screen::Mcp => {
            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(body_area);
            let selected_ids = state
                .working_copy
                .as_ref()
                .and_then(|c| c.selections.as_ref())
                .map(|s| s.mcp.clone())
                .unwrap_or_default();

            let mut list_state = ListState::default();
            let visible = state.filtered_mcp_indices();
            if !visible.is_empty() {
                let idx = if visible.contains(&state.mcp_selection_index) {
                    state.mcp_selection_index
                } else {
                    visible[0]
                };
                let selected_pos = visible.iter().position(|v| *v == idx).unwrap_or(0);
                list_state.select(Some(selected_pos));
            }

            let items: Vec<ListItem> = state
                .filtered_mcp_indices()
                .iter()
                .map(|index| {
                    let entry = &state.mcp_entries[*index];
                    let is_enabled = selected_ids.contains(&entry.id.to_string());
                    let enabled_marker = if is_enabled { "[x]" } else { "[ ]" };
                    ListItem::new(Line::from(vec![
                        Span::raw(enabled_marker),
                        Span::raw(" "),
                        Span::raw(entry.name.clone()),
                    ]))
                })
                .collect();

            let title = format!(
                "MCP Servers ({}/{}, shown {})",
                selected_ids.len(),
                state.mcp_entries.len(),
                visible.len()
            );
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(title))
                .highlight_symbol("› ")
                .highlight_style(Style::default().bg(theme.selection(navigation::mode(state))));
            f.render_stateful_widget(list, body_chunks[0], &mut list_state);

            if visible.is_empty() {
                f.render_widget(
                    Paragraph::new("No matching MCP entries. Press '/' to edit search.")
                        .block(Block::default().borders(Borders::ALL)),
                    body_chunks[1],
                );
            } else {
                let selected = if visible.contains(&state.mcp_selection_index) {
                    state.mcp_selection_index
                } else {
                    visible[0]
                };
                let current = &state.mcp_entries[selected];
                let kind = match current.source.kind {
                    macc_core::catalog::SourceKind::Git => "git",
                    macc_core::catalog::SourceKind::Http => "http",
                    macc_core::catalog::SourceKind::Local => "local",
                };
                let mut detail = format!(
                    "ID: {}\nName: {}\nKind: {}\n",
                    current.id, current.name, kind
                );
                detail.push_str("\nDescription:\n");
                detail.push_str(&current.description);
                detail.push_str("\n\nTags:\n");
                if current.tags.is_empty() {
                    detail.push_str("(none)");
                } else {
                    detail.push_str(&current.tags.join(", "));
                }
                detail.push_str(
                    "\n\nNotes:\n- MCP packages are merged into .mcp.json on apply.\n- Secrets are never stored by MACC.",
                );

                let desc_para = Paragraph::new(detail).block(panel("Details"));
                f.render_widget(desc_para, body_chunks[1]);
            }
        }
        Screen::Logs => {
            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
                .split(body_area);

            let mut list_state = ListState::default();
            let visible = state.filtered_log_indices();
            if !visible.is_empty() {
                let idx = if visible.contains(&state.log_selection_index) {
                    state.log_selection_index
                } else {
                    visible[0]
                };
                let selected_pos = visible.iter().position(|v| *v == idx).unwrap_or(0);
                list_state.select(Some(selected_pos));
            }
            let items: Vec<ListItem> = state
                .filtered_log_indices()
                .iter()
                .map(|index| {
                    let entry = &state.log_entries[*index];
                    ListItem::new(Line::from(vec![Span::raw(entry.relative.clone())]))
                })
                .collect();
            let list_title = format!(
                "Log Files (shown {}/{})",
                visible.len(),
                state.log_entries.len()
            );
            let list = List::new(items)
                .block(panel(&list_title))
                .highlight_symbol("› ")
                .highlight_style(Style::default().bg(theme.selection(navigation::mode(state))));
            f.render_stateful_widget(list, body_chunks[0], &mut list_state);

            let selected = state
                .log_entries
                .get(state.log_selection_index)
                .map(|e| e.path.display().to_string())
                .unwrap_or_else(|| "(none)".to_string());
            let content_title = format!("Content: {}", selected);
            let content = Paragraph::new(state.log_view_content.clone())
                .block(panel(&content_title))
                .scroll((state.log_content_scroll as u16, 0))
                .wrap(Wrap { trim: false });
            f.render_widget(content, body_chunks[1]);
        }
        Screen::Agents => {
            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(body_area);

            let selected_agents = state.selected_agents();

            let mut list_state = ListState::default();
            let visible = state.filtered_agent_indices();
            if !visible.is_empty() {
                let idx = if visible.contains(&state.agent_selection_index) {
                    state.agent_selection_index
                } else {
                    visible[0]
                };
                let selected_pos = visible.iter().position(|v| *v == idx).unwrap_or(0);
                list_state.select(Some(selected_pos));
            }

            let items: Vec<ListItem> = state
                .filtered_agent_indices()
                .iter()
                .map(|index| {
                    let agent = &state.agents[*index];
                    let is_enabled = selected_agents.contains(&agent.id.to_string());
                    let enabled_marker = if is_enabled { "[x]" } else { "[ ]" };
                    ListItem::new(Line::from(vec![
                        Span::raw(enabled_marker),
                        Span::raw(" "),
                        Span::raw(agent.name.clone()),
                    ]))
                })
                .collect();

            let title = format!(
                "Agents ({}/{}, shown {})",
                selected_agents.len(),
                state.agents.len(),
                visible.len()
            );
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(title))
                .highlight_symbol("› ")
                .highlight_style(Style::default().bg(theme.selection(navigation::mode(state))));
            f.render_stateful_widget(list, body_chunks[0], &mut list_state);

            if visible.is_empty() {
                f.render_widget(
                    Paragraph::new("No matching agents. Press '/' to edit search.")
                        .block(Block::default().borders(Borders::ALL)),
                    body_chunks[1],
                );
            } else {
                let selected = if visible.contains(&state.agent_selection_index) {
                    state.agent_selection_index
                } else {
                    visible[0]
                };
                let current_agent = &state.agents[selected];
                let mut desc_text = format!("ID: {}\n\n", current_agent.id);
                desc_text.push_str("Purpose:\n");
                desc_text.push_str(&current_agent.description);

                let desc_para = Paragraph::new(desc_text).block(panel("Details"));
                f.render_widget(desc_para, body_chunks[1]);
            }
        }
        Screen::Preview => preview_view::draw(f, state, body_area),
        Screen::Apply => state.draw_apply(f, body_area),
        Screen::Home => overview_view::draw(f, state, body_area),
        Screen::Sessions | Screen::O2 => state.sessions.draw(f, body_area),
        Screen::Hooks => state.hooks.draw(f, body_area),
        Screen::Automation => automation_view::draw(f, state, body_area, &draft),
        Screen::CoordinatorLive => coordinator_live::draw(f, state, body_area),
        Screen::Tools | Screen::ToolSettings => tools_view::draw(f, state, body_area),
        Screen::About => {
            let body = wrapped_paragraph(
                concat!(
                    "MACC ",
                    env!("CARGO_PKG_VERSION"),
                    "\n\nManage project configuration and coordinator runs."
                ),
                "About",
            );
            f.render_widget(body, body_area);
        }
        Screen::Watch => {
            render_watch_screen(f, state, body_area);
        }
    }

    draw_overlays(f, state);
}
fn draw_overlays(f: &mut Frame, state: &AppState) {
    if state.has_coordinator_pause_prompt() {
        render_coordinator_pause_overlay(f, state);
    }
    if state.has_coordinator_finished_prompt() {
        render_coordinator_finished_overlay(f, state);
    }
    if let Some(req) = state
        .coordinator_ownership
        .pending_incoming_request
        .as_ref()
    {
        crate::ownership::render_takeover_modal(f, ui::content_area(f.size()), req);
    }
    if state.current_screen() == Screen::CoordinatorLive {
        if state.coordinator_stop_dialog_open {
            render_coordinator_stop_dialog(f, state);
        }
        if state.coordinator_stop_task_confirm_id.is_some() {
            render_stop_task_confirm_dialog(f, state);
        }
        if state.coordinator_recover_dialog_open {
            render_coordinator_recover_dialog(f, state);
        }
    }
    if matches!(state.current_screen(), Screen::Sessions | Screen::O2) {
        state.sessions.draw_overlay(f);
    }
    if let Some(window) = &state.approval_window {
        window.draw(f);
    }
    state.decision_center.draw(f);
    state.draw_apply_confirmation(f);
    state.draw_launch_dialog(f);
    if state.command_palette_open {
        render_command_palette_overlay(f, state);
    }
    if state.help_open {
        render_help_overlay(f, state);
    }
    command_footer::draw(f, state);
}

fn kind_label(kind: PlannedOpKind) -> &'static str {
    match kind {
        PlannedOpKind::Write => "write",
        PlannedOpKind::Merge => "merge",
        PlannedOpKind::Delete => "delete",
        PlannedOpKind::Mkdir => "mkdir",
        PlannedOpKind::Other => "other",
    }
}

fn field_kind_label(kind: &FieldKind) -> String {
    match kind {
        FieldKind::Bool => "bool".to_string(),
        FieldKind::Enum(options) => format!("enum ({} options)", options.len()),
        FieldKind::Text => "text".to_string(),
        FieldKind::Number => "number".to_string(),
        FieldKind::Array => "array".to_string(),
        FieldKind::Action(_) => "action".to_string(),
    }
}

fn field_default_label(default: &Option<FieldDefault>) -> Option<String> {
    match default {
        Some(FieldDefault::Bool(value)) => Some(if *value { "On" } else { "Off" }.into()),
        Some(FieldDefault::Text(value)) => Some(value.clone()),
        Some(FieldDefault::Enum(value)) => Some(value.clone()),
        Some(FieldDefault::Number(value)) => Some(value.to_string()),
        Some(FieldDefault::Array(values)) => Some(values.join(", ")),
        None => None,
    }
}

fn scope_label(scope: Scope) -> &'static str {
    match scope {
        Scope::Project => "project",
        Scope::User => "user",
    }
}

fn tool_detail_text(
    id: &str,
    title: &str,
    description: &str,
    field_count: usize,
    status_label: &str,
    enabled: bool,
) -> String {
    let enabled_label = if enabled { "enabled" } else { "disabled" };
    let next = match (enabled, status_label) {
        (true, "installed") => "Enter to configure, Space to disable",
        (false, "installed") => "Space to enable, Enter to configure",
        (_, "missing") => "Press i to install, or choose another installed tool",
        (_, "error") => "Press d to refresh checks after fixing the error",
        _ => "Review tool details",
    };

    format!(
        "Current state: {} and {}\nNext: {}\n\nID: {}\nName: {}\nFields: {}\n\nDescription:\n{}\n",
        enabled_label, status_label, next, id, title, field_count, description
    )
}

fn coordinator_basics_text(state: &AppState) -> String {
    let tool = state.automation_field_display_value(0);
    let tool = if tool.is_empty() {
        "(auto-select)".to_string()
    } else {
        tool
    };
    let reference_branch = state.automation_field_display_value(1);
    let max_dispatch = state.automation_field_display_value(6);
    let max_parallel = state.automation_field_display_value(7);
    let safety_policy = state.automation_field_display_value(32);
    let destructive_actions = state.automation_field_display_value(33);

    format!(
        "Coordinator basics\nTool: {}\nParallelism: {} task(s), {} dispatch\nReference branch: {}\nSafety: {}, destructive actions require {}",
        tool,
        max_parallel,
        max_dispatch,
        reference_branch,
        safety_policy,
        destructive_actions.replace('_', " ")
    )
}

fn build_readiness_text(
    ladder: &macc_core::onboarding::ReadinessLadder,
    doctor_summary: Option<&str>,
) -> String {
    // If a doctor check has been run, show its detailed output instead of the ladder.
    if let Some(summary) = doctor_summary {
        let mut out = summary.to_string();
        out.push_str("\nRecommended: fix the blocking issue(s) above, then [d] re-check.");

        return out;
    }

    let mut out = String::new();
    out.push_str("MACC readiness\n\n");
    let next = overview_view::next_step(ladder);
    for (index, step) in ladder.steps.iter().enumerate() {
        let (symbol, _) = overview_view::status(step, Some(index) == next);
        let detail = step.detail.as_deref().unwrap_or("");
        let fix = if matches!(step.status, macc_core::onboarding::ReadinessStatus::Pending) {
            readiness_step_fix(step.number)
        } else {
            None
        };
        if detail.is_empty() {
            out.push_str(&format!(
                "{}. {}  {}{}\n",
                step.number,
                step.label,
                symbol,
                fix.map(|fix| format!("  Fix: {}", fix.summary))
                    .unwrap_or_default()
            ));
        } else {
            out.push_str(&format!(
                "{}. {}  {}  {}{}\n",
                step.number,
                step.label,
                symbol,
                detail,
                fix.map(|fix| format!("  Fix: {}", fix.summary))
                    .unwrap_or_default()
            ));
        }
        if let Some(fix) = fix {
            out.push_str(&format!("   Action: {}\n", fix.action));
        }
    }
    out.push('\n');
    if ladder.is_ready() {
        out.push_str("✓ Ready to dispatch a task\n\n");
        out.push_str("Recommended: [r] Start coordinator\n");
        out.push_str("Why: all required setup checks are green.\n\n");
    } else {
        out.push_str(&format!("▲ {} step(s) pending\n\n", ladder.blocking_count));
        let recommendation = readiness_recommendation(ladder);
        out.push_str("Recommended next step:\n");
        out.push_str(&format!("  {}\n", recommendation.action));
        out.push_str(&format!("  Why: {}\n\n", recommendation.reason));
        out.push_str("Other actions:\n");
        out.push_str("  [d] Doctor check\n");
        for action in recommendation.other_actions {
            out.push_str(&format!("  {}\n", action));
        }
        out.push_str("  [?] All Keybindings\n");
    }
    out
}

fn next_step_text(state: &AppState, max_chars: usize) -> String {
    if state.project_paths.is_none() {
        return ui::truncate_middle(
            "Next step: Initialize this project. Run `macc init`.",
            max_chars,
        );
    };

    if let Some(run) = state.unresolved_run() {
        return ui::truncate_middle(
            &format!(
                "Last run {}. {}  C Coordinator live",
                run.status, run.headline
            ),
            max_chars,
        );
    }
    let Some(obs) = &state.observations.latest else {
        return "Checking project readiness…".into();
    };
    let text = next_step_text_for_ladder(&obs.readiness, state.is_coordinator_running());
    let text = if state.current_screen() == Screen::CoordinatorLive {
        text.replace(
            "Press r to start coordinator.",
            "Open Commands to start the coordinator.",
        )
        .replace(
            "Open Changes to review Apply.",
            "Preview and apply configuration.",
        )
        .replace(
            "Open Coordinator to watch live tasks.",
            "Live task status appears below.",
        )
    } else {
        text.to_string()
    };
    ui::truncate_middle(&text, max_chars)
}

fn next_step_text_for_ladder(
    ladder: &macc_core::onboarding::ReadinessLadder,
    coordinator_running: bool,
) -> &'static str {
    let first_pending = ladder
        .steps
        .iter()
        .find(|step| matches!(step.status, macc_core::onboarding::ReadinessStatus::Pending));

    match first_pending.map(|step| step.number) {
        Some(1) => "Next step: Project not initialized. Run `macc init`.",
        Some(2) => "Next step: Tool missing. Open Configure › Tools to select a performer tool.",
        Some(3) => "Next step: Config not applied. Open Changes to review Apply.",
        Some(4) => {
            "Next step: PRD/task missing. Run `macc prd generate --from <brief.md> --promote`."
        }
        Some(5) => "Next step: Git identity missing. Configure git user.name and user.email.",
        Some(6) => "Next step: Ready to run. Press r to start coordinator.",
        Some(_) => "Next step: Run Doctor. Press d from Home to inspect the blocking setup step.",
        None if coordinator_running => {
            "Next step: Coordinator is running. Open Coordinator to watch live tasks."
        }
        None => "Next step: Ready to run. Press r to start coordinator.",
    }
}

fn trust_warning_strip(
    paths: &macc_core::ProjectPaths,
    config: &macc_core::config::CanonicalConfig,
) -> Option<String> {
    let trust = macc_core::ops_motif::calculate_trust_summary(paths, config);
    let mut warnings = Vec::new();

    if trust.terminal_enabled {
        warnings.push("terminal allowed".to_string());
    }
    if trust.user_level_writes > 0 {
        warnings.push(format!("{} user-file write(s)", trust.user_level_writes));
    }
    if !trust.backups_ready {
        warnings.push("backups missing".to_string());
    }
    if !trust.catalog_pinned {
        warnings.push("catalog unpinned".to_string());
    }
    if !trust.secrets_redacted {
        warnings.push("secrets visible".to_string());
    }

    if warnings.is_empty() {
        None
    } else {
        Some(warnings.join(" | "))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadinessRecommendation {
    action: &'static str,
    reason: &'static str,
    other_actions: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
struct ReadinessStepFix {
    summary: &'static str,
    action: &'static str,
}

fn readiness_step_fix(number: u8) -> Option<ReadinessStepFix> {
    match number {
        1 => Some(ReadinessStepFix {
            summary: "initialize this project",
            action: "CLI: macc init",
        }),
        2 => Some(ReadinessStepFix {
            summary: "select a performer tool",
            action: "Open Configure › Tools",
        }),
        3 => Some(ReadinessStepFix {
            summary: "apply the selected configuration",
            action: "Open Configure › Changes to review Apply",
        }),
        4 => Some(ReadinessStepFix {
            summary: "create/promote a PRD task",
            action: "Review the PRD queue, or CLI: macc prd generate --from brief.md",
        }),
        5 => Some(ReadinessStepFix {
            summary: "configure git identity",
            action: "CLI: git config user.name <name> && git config user.email <email>",
        }),
        6 => Some(ReadinessStepFix {
            summary: "start the coordinator",
            action: "Press r to start coordinator",
        }),
        _ => None,
    }
}

fn readiness_recommendation(
    ladder: &macc_core::onboarding::ReadinessLadder,
) -> ReadinessRecommendation {
    let first_pending = ladder
        .steps
        .iter()
        .find(|step| matches!(step.status, macc_core::onboarding::ReadinessStatus::Pending));

    match first_pending.map(|step| step.number) {
        Some(1) => ReadinessRecommendation {
            action: "CLI: macc init",
            reason: "this folder is not initialized as a MACC project yet.",
            other_actions: &["CLI: macc quickstart"],
        },
        Some(2) => ReadinessRecommendation {
            action: "Open Configure › Tools and select one tool",
            reason: "MACC needs a performer tool before it can run tasks.",
            other_actions: &["Configure › Changes › Apply", "[C] Coordinator live view"],
        },
        Some(3) => ReadinessRecommendation {
            action: "Configure › Changes › Apply",
            reason: "the selected configuration has not been written to managed files yet.",
            other_actions: &["Configure › Tools", "[d] Doctor check"],
        },
        Some(4) => ReadinessRecommendation {
            action: "Create or import a PRD at .macc/prd.json",
            reason: "the coordinator cannot dispatch work until at least one task is ready. Starting it now will not create tasks.",
            other_actions: &[
                "CLI: macc prd generate --from <brief.md> --promote",
                "[d] Doctor check",
                "[C] Coordinator live view",
            ],
        },
        Some(5) => ReadinessRecommendation {
            action: "Configure git user.name and user.email",
            reason: "MACC needs a commit identity before performers can safely create commits.",
            other_actions: &["CLI: git config user.name <name>", "CLI: git config user.email <email>"],
        },
        Some(6) => ReadinessRecommendation {
            action: "[r] Start coordinator",
            reason: "setup and tasks are ready; only the coordinator process is stopped.",
            other_actions: &["[C] Coordinator live view", "[d] Doctor check"],
        },
        _ => ReadinessRecommendation {
            action: "[d] Doctor check",
            reason: "MACC needs more detail before choosing a safe action.",
            other_actions: &["Configure › Changes › Apply", "[C] Coordinator live view"],
        },
    }
}

fn coordinator_live_empty_lines(
    no_run_and_no_tasks: bool,
    search_active: bool,
    total_tasks: usize,
) -> Vec<Line<'static>> {
    if no_run_and_no_tasks {
        return vec![
            Line::from("No tasks available"),
            Line::from("Reason: no PRD/task registry entries are ready"),
            Line::from("Next: create/promote a PRD task, then start the coordinator."),
        ];
    }

    if search_active {
        return vec![
            Line::from("No visible tasks match the current search."),
            Line::from("Next: clear or adjust search to see existing coordinator tasks."),
        ];
    }

    if total_tasks > 0 {
        return vec![
            Line::from("No live task is currently active."),
            Line::from("Next: start or resume the coordinator to dispatch ready work."),
        ];
    }

    vec![
        Line::from("No live task is currently active."),
        Line::from("Next: run Doctor from Home to find the blocking setup step."),
    ]
}

fn render_watch_screen(f: &mut Frame, state: &AppState, area: Rect) {
    let theme = theme();
    let snapshot = state.watch_snapshot.as_ref();

    let control_label = if state.watch_control_enabled {
        " [CONTROL]"
    } else {
        " [READ-ONLY]"
    };
    let age_label = state
        .watch_last_refresh
        .map(|ts| {
            let secs = ts.elapsed().as_secs();
            if secs < 5 {
                "now".to_string()
            } else {
                format!("{}s ago", secs)
            }
        })
        .unwrap_or_else(|| "loading…".to_string());
    let title = format!("Observer{} · refreshed {}", control_label, age_label);

    // Spec §2.12: paused coordinator banner — displayed above everything when paused.
    let is_paused = snapshot.map(|s| s.coordinator.paused).unwrap_or(false);
    let pause_reason = snapshot
        .and_then(|s| s.coordinator.pause_reason.as_deref())
        .unwrap_or("operator requested pause");

    let (banner_area, content_area) = if is_paused {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(area);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, area)
    };

    if let Some(ba) = banner_area {
        let banner = Paragraph::new(format!("  ⚠  COORDINATOR PAUSED — {} ", pause_reason)).style(
            Style::default()
                .fg(crate::ui::theme().background)
                .bg(crate::ui::theme().warn)
                .add_modifier(Modifier::BOLD),
        );
        f.render_widget(banner, ba);
    }

    // Build the vertical layout depending on --logs-only / --events-only filter flags.
    let (show_top, show_log) = if state.watch_logs_only {
        (false, true)
    } else if state.watch_events_only {
        (true, false)
    } else {
        (true, true)
    };

    let vertical_constraints: Vec<Constraint> = match (show_top, show_log) {
        (true, true) => vec![
            Constraint::Percentage(40),
            Constraint::Percentage(30),
            Constraint::Min(3),
        ],
        (true, false) => vec![
            Constraint::Percentage(80),
            Constraint::Length(0),
            Constraint::Min(3),
        ],
        (false, true) => vec![
            Constraint::Length(0),
            Constraint::Percentage(90),
            Constraint::Min(3),
        ],
        (false, false) => vec![
            Constraint::Percentage(40),
            Constraint::Percentage(30),
            Constraint::Min(3),
        ],
    };

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vertical_constraints)
        .split(content_area);

    // Top row (workers / queue / events) — hidden in --logs-only mode.
    if show_top {
        let top_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(35),
                Constraint::Percentage(40),
                Constraint::Percentage(25),
            ])
            .split(vertical[0]);

        // Workers pane — spec §6.4 WorkerRuntime.
        // Stale detection: heartbeat age > 180s triggers ▲ independently of runtime_status
        // (spec §2.6 says "freshly-stale workers" must be detected by timestamp, not just
        // coordinator-assigned status).
        let now_rfc = chrono::Utc::now();
        let worker_lines: Vec<ListItem> = if let Some(snap) = snapshot {
            if snap.workers.is_empty() {
                vec![ListItem::new("No active workers")]
            } else {
                snap.workers
                    .iter()
                    .enumerate()
                    .map(|(i, w)| {
                        let sel = i == state.watch_selected_worker;

                        // Compute staleness from the raw ISO-8601 timestamp.
                        let freshly_stale = w
                            .last_heartbeat
                            .as_deref()
                            .and_then(|hb| chrono::DateTime::parse_from_rfc3339(hb).ok())
                            .map(|hb| {
                                (now_rfc - hb.with_timezone(&chrono::Utc)).num_seconds() > 180
                            })
                            .unwrap_or(false);

                        let symbol = if freshly_stale
                            || w.runtime_status == "stale"
                            || w.runtime_status == "failed"
                        {
                            "▲"
                        } else {
                            "●"
                        };

                        let phase = w.phase.as_deref().unwrap_or("-");
                        let task = w.task_id.as_deref().unwrap_or("-");
                        let hb_age = w
                            .last_heartbeat
                            .as_deref()
                            .and_then(|hb| chrono::DateTime::parse_from_rfc3339(hb).ok())
                            .map(|hb| {
                                let secs = (now_rfc - hb.with_timezone(&chrono::Utc)).num_seconds();
                                if secs < 60 {
                                    format!("{}s", secs)
                                } else {
                                    format!("{}m", secs / 60)
                                }
                            })
                            .unwrap_or_else(|| "-".to_string());

                        let text = format!(
                            "{} {}  {}  {}  {}  hb {}",
                            symbol, w.id, task, phase, w.runtime_status, hb_age
                        );
                        let style = if sel {
                            Style::default()
                                .fg(theme.accent)
                                .add_modifier(Modifier::BOLD)
                        } else if freshly_stale || symbol == "▲" {
                            Style::default().fg(crate::ui::theme().warn)
                        } else {
                            Style::default()
                        };
                        ListItem::new(text).style(style)
                    })
                    .collect()
            }
        } else {
            vec![ListItem::new("Waiting for snapshot… (refreshes every 2s)")]
        };
        let workers_list = List::new(worker_lines).block(
            Block::default()
                .title("Workers  ↑↓ navigate")
                .borders(Borders::ALL),
        );
        f.render_widget(workers_list, top_chunks[0]);

        // Queue / git pane — spec §6.5 QueueSummary.
        let tasks_text = if let Some(snap) = snapshot {
            let q = &snap.queue;
            format!(
                "todo {}  active {}  blocked {}  merged {}  total {}\n\nbranch: {}",
                q.todo,
                q.in_progress,
                q.blocked,
                q.merged,
                q.total,
                snap.git.current_branch.as_deref().unwrap_or("-"),
            )
        } else {
            "No snapshot available — coordinator may not be running.".to_string()
        };
        let tasks_para = wrapped_paragraph(&tasks_text, "Queue / Git");
        f.render_widget(tasks_para, top_chunks[1]);

        // Events pane — spec §6.2 RuntimeEvent.
        let events_text = if let Some(snap) = snapshot {
            snap.recent_events
                .iter()
                .rev()
                .take(12)
                .rev()
                .map(|ev| {
                    let ts = ev.ts.as_deref().unwrap_or("").get(11..19).unwrap_or("");
                    let task = ev.task_id.as_deref().unwrap_or("");
                    format!("{} {} {}", ts, ev.event_type, task)
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };
        let events_para = wrapped_paragraph(
            if events_text.is_empty() {
                "No events"
            } else {
                &events_text
            },
            "Events",
        );
        f.render_widget(events_para, top_chunks[2]);
    }

    // Log pane.
    if show_log {
        let log_text = state
            .watch_log_tail
            .iter()
            .rev()
            .take(15)
            .rev()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let log_para = wrapped_paragraph(
            if log_text.is_empty() {
                "No log tail is available."
            } else {
                &log_text
            },
            "Coordinator Log",
        );
        f.render_widget(log_para, vertical[1]);
    }

    // Status strip — spec §6.6 ToolThrottleStatus.
    let throttled_label = if let Some(snap) = snapshot {
        if snap.throttled_tools.is_empty() {
            String::new()
        } else {
            let names: Vec<_> = snap
                .throttled_tools
                .iter()
                .map(|t| format!("{} throttled ({}s)", t.tool, t.backoff_seconds))
                .collect();
            format!(" | {}", names.join(", "))
        }
    } else {
        String::new()
    };
    let strip_text = format!("{}{}", title, throttled_label);
    let strip =
        Paragraph::new(strip_text).block(Block::default().title("Status").borders(Borders::ALL));
    f.render_widget(strip, vertical[2]);
}

fn render_help_overlay(f: &mut Frame, state: &AppState) {
    let area = ui::centered_rect(85, 85, ui::content_area(f.size()));
    f.render_widget(Clear, area); // Clear the background

    let current_screen = state.current_screen();
    let help_items = command_catalog::active(state).into_iter().map(|c| {
        (
            c.shortcut(),
            format!(
                "{}{}",
                c.label,
                if c.enabled { "" } else { " [unavailable]" }
            ),
        )
    });

    let mut text = format!("Help: {}\n\n", current_screen.title());
    for (key, desc) in help_items {
        text.push_str(&format!("[{}] {}\n", key, desc));
    }

    let help_para = Paragraph::new(text)
        .scroll((state.help_scroll, 0))
        .block(
            Block::default()
                .title("Help — current page / window")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(crate::ui::theme().accent)),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(help_para, area);
}

fn render_coordinator_ownership_banner(f: &mut Frame, area: Rect, state: &AppState) {
    use crate::ownership::{render_ownership_banner, OwnershipBannerProps};

    let (owner_label, viewer_count) = match state.coordinator_ownership.record.as_ref() {
        Some(r) => {
            let owner = r
                .owner
                .as_ref()
                .map(|o| o.client_id.clone())
                .unwrap_or_else(|| "<none>".to_string());
            (owner, r.viewers.len())
        }
        None => ("<none>".to_string(), 0usize),
    };
    let has_pending_request = state
        .coordinator_ownership
        .pending_incoming_request
        .is_some();

    render_ownership_banner(
        f,
        area,
        &OwnershipBannerProps {
            owner_label,
            viewer_count,
            is_owner: state.coordinator_ownership.is_owner,
            has_pending_request,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use macc_core::onboarding::{ReadinessLadder, ReadinessStatus, ReadinessStep};
    use macc_core::tool::ToolRegistry;
    use macc_core::MaccEngine;
    use std::sync::Arc;

    fn step(number: u8, label: &str, status: ReadinessStatus) -> ReadinessStep {
        ReadinessStep {
            number,
            label: label.to_string(),
            status,
            detail: None,
        }
    }

    #[test]
    fn readiness_recommends_prd_before_starting_coordinator() {
        let ladder = ReadinessLadder {
            steps: vec![
                step(1, "Project initialized", ReadinessStatus::Done),
                step(2, "Tool adapter selected", ReadinessStatus::Done),
                step(3, "Config applied", ReadinessStatus::Done),
                step(4, "PRD/task available", ReadinessStatus::Pending),
                step(5, "Git identity configured", ReadinessStatus::Done),
                step(6, "Coordinator running", ReadinessStatus::Pending),
            ],
            blocking_count: 2,
        };

        let text = build_readiness_text(&ladder, None);

        assert!(text.contains("Recommended next step:"));
        assert!(text.contains("Create or import a PRD at .macc/prd.json"));
        assert!(text.contains("Starting it now will not create tasks"));
    }

    #[test]
    fn readiness_checklist_shows_actionable_fix_for_missing_prd() {
        let ladder = ReadinessLadder {
            steps: vec![
                step(1, "Project initialized", ReadinessStatus::Done),
                step(4, "PRD/task available", ReadinessStatus::Pending),
            ],
            blocking_count: 1,
        };

        let text = build_readiness_text(&ladder, None);

        assert!(text.contains("4. PRD/task available  ▲  Fix: create/promote a PRD task"));
        assert!(text
            .contains("Action: Review the PRD queue, or CLI: macc prd generate --from brief.md"));
        assert!(!text.contains("1. Project initialized  ✓  Fix:"));
    }

    #[test]
    fn readiness_recommends_starting_coordinator_only_after_prd_exists() {
        let ladder = ReadinessLadder {
            steps: vec![
                step(1, "Project initialized", ReadinessStatus::Done),
                step(2, "Tool adapter selected", ReadinessStatus::Done),
                step(3, "Config applied", ReadinessStatus::Done),
                step(4, "PRD/task available", ReadinessStatus::Done),
                step(5, "Git identity configured", ReadinessStatus::Done),
                step(6, "Coordinator running", ReadinessStatus::Pending),
            ],
            blocking_count: 1,
        };

        let recommendation = readiness_recommendation(&ladder);

        assert_eq!(recommendation.action, "[r] Start coordinator");
        assert!(recommendation.reason.contains("tasks are ready"));
    }

    #[test]
    fn coordinator_live_empty_state_explains_next_prd_action() {
        let lines = coordinator_live_empty_lines(true, false, 0);
        let text = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("No tasks available"));
        assert!(text.contains("Reason: no PRD/task registry entries are ready"));
        assert!(text.contains("Next: create/promote a PRD task, then start the coordinator."));
        assert!(!text.contains("Useful:"));
    }

    #[test]
    fn coordinator_live_empty_state_respects_search_filter() {
        let lines = coordinator_live_empty_lines(false, true, 3);
        let text = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("current search"));
        assert!(!text.contains("create or promote a PRD"));
    }

    #[test]
    fn next_step_band_prioritizes_missing_prd() {
        let ladder = ReadinessLadder {
            steps: vec![
                step(1, "Project initialized", ReadinessStatus::Done),
                step(2, "Tool adapter selected", ReadinessStatus::Done),
                step(3, "Config applied", ReadinessStatus::Done),
                step(4, "PRD/task available", ReadinessStatus::Pending),
                step(6, "Coordinator running", ReadinessStatus::Pending),
            ],
            blocking_count: 2,
        };

        assert_eq!(
            next_step_text_for_ladder(&ladder, false),
            "Next step: PRD/task missing. Run `macc prd generate --from <brief.md> --promote`."
        );
    }

    #[test]
    fn next_step_band_recommends_run_when_setup_is_ready() {
        let ladder = ReadinessLadder {
            steps: vec![
                step(1, "Project initialized", ReadinessStatus::Done),
                step(2, "Tool adapter selected", ReadinessStatus::Done),
                step(3, "Config applied", ReadinessStatus::Done),
                step(4, "PRD/task available", ReadinessStatus::Done),
                step(5, "Git identity configured", ReadinessStatus::Done),
                step(6, "Coordinator running", ReadinessStatus::Pending),
            ],
            blocking_count: 1,
        };

        assert_eq!(
            next_step_text_for_ladder(&ladder, false),
            "Next step: Ready to run. Press r to start coordinator."
        );
    }

    #[test]
    fn compact_header_exposes_mode_and_real_attention_without_idle_status_rows() {
        let mut ctx = HeaderContext {
            mode: navigation::Mode::Configure,
            page: "Changes",
            project: "example",
            config_label: "ok",
            errors: 0,
            inbox: 1,
            draft_count: 2,
            coordinator_active: false,
            coordinator_paused: false,
            coordinator_stopped: false,
            action_needed: false,
            next_step: "",
            status: None,
            width: 120,
            trust_strip: None,
            override_strip: None,
        };
        let lines = header_lines(&ctx, &theme());
        assert_eq!(lines.len(), 1);
        assert!(lines[0].to_string().contains("inbox ▲ 1  ◆ 2 unsaved"));
        assert!(lines[0].to_string().contains("CONFIGURE"));
        ctx.trust_strip = Some("terminal allowed".into());
        ctx.override_strip = Some("supervisor waiting".into());
        let lines = header_lines(&ctx, &theme());
        assert!(lines[1].to_string().contains("terminal allowed"));
        assert!(lines[2].to_string().contains("supervisor waiting"));
    }
    #[test]
    fn command_palette_filters_available_commands() {
        let engine = Arc::new(MaccEngine::new(ToolRegistry::new()));
        let mut state = AppState::with_engine(engine);
        state.command_palette_query = "skill".into();
        let items = contextual_items(&state);

        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].action,
            command_catalog::Action::Go(navigation::Destination::Skills)
        );
        assert_eq!(items[0].title, "Skills");
    }

    #[test]
    fn command_palette_executes_navigation_action() {
        let engine = Arc::new(MaccEngine::new(ToolRegistry::new()));
        let mut state = AppState::with_engine(engine);

        command_catalog::execute(
            &mut state,
            command_catalog::Action::Go(navigation::Destination::Mcp),
        );

        assert_eq!(state.current_screen(), Screen::Mcp);
    }

    #[test]
    fn installed_tool_detail_prioritizes_current_state_and_next_action() {
        let text = tool_detail_text(
            "agy",
            "Antigravity",
            "Workspace settings for Google Antigravity CLI.",
            7,
            "installed",
            true,
        );

        assert!(text.starts_with("Current state: enabled and installed"));
        assert!(text.contains("Next: Enter to configure, Space to disable"));
        assert!(!text.contains("Install:"));
    }

    #[test]
    fn missing_tool_detail_points_to_install() {
        let text = tool_detail_text(
            "claude",
            "Claude",
            "Claude CLI settings.",
            4,
            "missing",
            false,
        );

        assert!(text.starts_with("Current state: disabled and missing"));
        assert!(text.contains("Next: Press i to install"));
    }

    #[test]
    fn coordinator_basics_summary_is_basic_first() {
        let engine = Arc::new(MaccEngine::new(ToolRegistry::new()));
        let state = AppState::with_engine(engine);

        let text = coordinator_basics_text(&state);

        assert!(text.starts_with("Coordinator basics"));
        assert!(text.contains("Tool:"));
        assert!(text.contains("Parallelism:"));
        assert!(text.contains("Reference branch:"));
        assert!(text.contains("Safety:"));
    }
}

/// Text of the approval block in Coordinator Live: one entry per waiting gate
/// with subject, revision, risks, approvers and the command to decide.
/// `(line, emphasised)`; pure so the wording is testable.
fn approval_request_lines(
    pending: &[macc_core::coordinator::approval::PendingApproval],
) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for p in pending {
        out.push((
            format!(
                "{} waits for human approval [{}] — no performer runs, no worker is used",
                p.task_id, p.status
            ),
            true,
        ));
        out.push((
            format!(
                "  Subject:   {} at {}",
                p.subject_task,
                p.subject_revision.as_deref().unwrap_or("unknown revision")
            ),
            false,
        ));
        if let Some(d) = p.description.as_deref() {
            out.push((format!("  Decision:  {d}"), false));
        }
        for risk in &p.risks {
            out.push((format!("  Risk:      {risk}"), false));
        }
        out.push((format!("  Approvers: {}", p.required), false));
        if let Some(note) = p.note.as_deref() {
            out.push((format!("  Status:    {note}"), false));
        }
        if let Some(stat) = p.diff_stat.as_deref() {
            for line in stat.lines().take(8) {
                out.push((format!("  {line}"), false));
            }
        }
        if p.awaiting_decision {
            out.push((
                "  Decide:    open Inbox to review the full request".to_string(),
                false,
            ));
        }
        out.push((format!("  CLI:       {}", p.approve_command), false));
    }
    out
}

fn render_coordinator_pause_overlay(f: &mut Frame, state: &AppState) {
    let area = ui::centered_rect(75, 45, ui::content_area(f.size()));
    f.render_widget(Clear, area);
    let diagnostic_label = state.coordinator_run_summary.as_ref().and_then(|s|s.diagnostic_delivery.as_ref()).map(|_| "Diagnostic report complete · feature changes requested · external verification pending\n").unwrap_or("");
    let message = state
        .coordinator_pause_error
        .as_deref()
        .unwrap_or("Coordinator paused due to an error.");
    let message_with_status = format!("{diagnostic_label}{message}");
    let message = message_with_status.as_str();
    let command_name = state.coordinator_pause_command.as_deref().unwrap_or("run");
    let summary = state.coordinator_run_summary.as_ref();
    let task = state
        .coordinator_pause_task_id
        .as_deref()
        .filter(|v| *v != "-" && !v.is_empty())
        .or_else(|| summary.and_then(|s| s.task_id.as_deref()));
    let phase = state
        .coordinator_pause_phase
        .as_deref()
        .filter(|p| *p != "run")
        .or_else(|| {
            summary
                .filter(|s| s.task_id.as_deref() == task)
                .and_then(|s| s.phase.as_deref())
        });
    let retry_target = match (task, phase) {
        (Some(task), Some(phase)) => format!("task={task} phase={phase}"),
        (Some(task), None) => format!("task={task} phase=dev"),
        _ => "global/blocking (no task context)".into(),
    };
    let (title, text, border) = pause_overlay_content(
        message,
        &retry_target,
        command_name,
        summary.and_then(|s| s.error_code.as_deref()),
        summary.and_then(|s| s.task_id.as_deref()),
        summary
            .map(|s| s.unmet_preconditions.as_slice())
            .unwrap_or(&[]),
        summary.map(|s| s.next_action.as_str()),
    );
    let popup = Paragraph::new(text)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border)),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(popup, area);
}

/// Title, body and border colour of the pause/stop overlay.
///
/// Pure so the wording can be unit-tested. A `precondition_unmet` stop (E903)
/// is *not* an error: the tool did exactly what the task specified. It gets
/// its own title and a body that lists the unsatisfied conditions instead of
/// the retry/skip menu, which would only re-run the same correct refusal.
fn precondition_stop(message: &str, code: Option<&str>, unmet: &[String]) -> bool {
    code == Some("E903") || message.contains("[E903]") || !unmet.is_empty()
}
fn pause_overlay_content(
    message: &str,
    retry_target: &str,
    command_name: &str,
    error_code: Option<&str>,
    blocked_task_id: Option<&str>,
    unmet_preconditions: &[String],
    next_action: Option<&str>,
) -> (&'static str, String, Color) {
    // Recorded preconditions decide the overlay, whichever code the failure
    // was normalized to: the retry menu would only repeat the same refusal.
    let is_precondition_stop = precondition_stop(message, error_code, unmet_preconditions);
    if is_precondition_stop
        && (message.contains("DM_STALE:")
            || unmet_preconditions.iter().any(|p| p.contains("DM_STALE:")))
        && !message.contains("DM_PENDING:")
        && !message.contains("DM_EXTERNAL_VERIFICATION:")
    {
        let body = format!("{}: the retained phase needs an updated decision context. Work is preserved; completion has not been accepted.\n\n{}\n\nNext: {}\n\n- Press 'o': open Logs and inspect checkpoint recovery\n- Press Esc: close and keep the run stopped\n\nCommand: {}",blocked_task_id.unwrap_or("The retained task"),message.trim(),next_action.unwrap_or("Use native checkpoint recovery to refresh this phase; inspect any refused checkpoint or changed field before retrying."),command_name);
        return (
            "Coordinator Paused — Context Refresh",
            body,
            crate::ui::theme().warn,
        );
    }
    if is_precondition_stop {
        let task = blocked_task_id.unwrap_or("The blocked task");
        let mut body = format!(
            "{} cannot be implemented because the following precondition{} not satisfied:\n",
            task,
            if unmet_preconditions.len() == 1 {
                " is"
            } else {
                "s are"
            }
        );
        if unmet_preconditions.is_empty() {
            // No structured list: fall back to the recorded message, which
            // already carries the tool's explanation.
            body.push_str(&format!("\n{}\n", message.trim()));
        } else {
            for item in unmet_preconditions {
                body.push_str(&format!("  - {}\n", item));
            }
        }
        body.push_str(
            "\nThis is not a tool failure: the task stopped as its specification requires. Retrying unchanged will stop again.\n",
        );
        if let Some(action) = next_action {
            body.push_str(&format!("\nNext: {}\n", action));
        }
        if macc_core::coordinator::phase_diagnostics::decision_precondition(message)
            || unmet_preconditions
                .iter()
                .any(|p| macc_core::coordinator::phase_diagnostics::decision_precondition(p))
        {
            body.push_str("\n- Press 'd': open Decisions and inspect the failed check\n");
        }
        body.push_str(
            "\n- Press 'o': open Logs screen\n- Press Esc: close and keep the run stopped\n",
        );
        body.push_str(&format!("\nCommand: {}\n", command_name));
        return (
            if message.contains("DM_EXTERNAL_VERIFICATION:") {
                "Coordinator Waiting — External Verification"
            } else {
                "Coordinator Blocked — Preconditions Not Satisfied"
            },
            body,
            crate::ui::theme().warn,
        );
    }
    // RL-TUI-007: show a specific banner when quota is exhausted (E602).
    let is_quota_error = message.contains("quota_exhausted")
        || message.contains("E602")
        || message.to_ascii_lowercase().contains("quota exhausted");
    let (title, text) = if is_quota_error {
        (
            "Coordinator Paused — Quota Exhausted",
            format!(
                "PAUSED: Quota exhausted.\n\n{}\n\nTarget:\n- {}\n\nOptions:\n- Wait for quota reset, then press 'u' to resume\n- Switch to another tool in your macc config, then press 'u'\n- Press 'r' or Enter: retry failed phase\n- Press 's': skip failed phase (move task to todo)\n- Press Esc: stop and keep paused state\n\nCommand: {}\n",
                message, retry_target, command_name
            ),
        )
    } else {
        (
            "Coordinator Error",
            format!(
                "Coordinator Paused (blocking error)\n\n{}\n\nTarget:\n- {}\n\nFix the issue in your repo/worktree, then choose:\n\n- Press 'r' or Enter: retry failed phase, then resume run\n- Press 's': skip failed phase (move task to todo), then resume run\n- Press 'u': send manual resume signal (same as `macc coordinator resume`)\n- Press 'o': open Logs screen\n- Press Esc: stop and keep paused state\n- Press 'c': resume run without retry\n\nCommand: {}\n",
                message, retry_target, command_name
            ),
        )
    };
    (title, text, crate::ui::theme().bad)
}

fn render_coordinator_finished_overlay(f: &mut Frame, state: &AppState) {
    let area = ui::centered_rect(70, 35, ui::content_area(f.size()));
    f.render_widget(Clear, area);
    let theme = ui::theme();
    let message = state
        .coordinator_finished_message
        .as_deref()
        .unwrap_or("Coordinator run finished.");
    let text = format!(
        "Coordinator run complete (no errors).\n\n{}\n\nPress Enter or Esc to exit.\n",
        message
    );
    let popup = Paragraph::new(text)
        .block(
            Block::default()
                .title("Coordinator Finished")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.good)),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(popup, area);
}

fn render_coordinator_stop_dialog(f: &mut Frame, state: &AppState) {
    let area = ui::centered_rect(65, 40, ui::content_area(f.size()));
    f.render_widget(Clear, area);
    let theme = ui::theme();

    let options = [
        "Drain & Stop: Finish active tasks, then stop.",
        "Graceful Stop: Stop active performers at next safe boundary.",
        "Force Stop: Terminate processes immediately.",
        "Force Stop + Cleanup: Terminate and delete worktrees/branches.",
    ];

    let mut text = vec![
        Line::from("Stopping the coordinator affects the whole run."),
        Line::from(
            "Impact depends on the mode: active performers may finish, pause, or be killed.",
        ),
        Line::from(""),
        Line::from("Select a stop mode:"),
        Line::from(""),
    ];

    for (idx, opt) in options.iter().enumerate() {
        let style = if idx == state.coordinator_stop_dialog_selection {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
                .bg(theme.selection(navigation::mode(state)))
        } else {
            Style::default()
        };
        let prefix = if idx == state.coordinator_stop_dialog_selection {
            "> "
        } else {
            "  "
        };
        text.push(Line::from(vec![Span::styled(
            format!("{}{}", prefix, opt),
            style,
        )]));
    }

    text.push(Line::from(""));
    text.push(Line::from("Controls:"));
    text.push(Line::from("- ↑↓: Move selection"));
    text.push(Line::from("- Enter: Confirm and apply"));
    text.push(Line::from("- Esc or q/n: Cancel"));

    let popup = Paragraph::new(text)
        .block(
            Block::default()
                .title("Coordinator Stop Modes")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent)),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(popup, area);
}

fn render_stop_task_confirm_dialog(f: &mut Frame, state: &AppState) {
    let area = ui::centered_rect(65, 32, ui::content_area(f.size()));
    f.render_widget(Clear, area);
    let theme = ui::theme();
    let task_id = state
        .coordinator_stop_task_confirm_id
        .as_deref()
        .unwrap_or("<unknown>");
    let text = format!(
        "Stop selected task?\n\nTask: {}\n\nImpact:\n- Sends a kill request for this task worker.\n- The task may be moved out of active execution.\n- Unmerged work in its worktree may remain for recovery/retry.\n\nConfirm:\n- Enter: stop this task\n- Esc: cancel",
        task_id
    );
    let popup = Paragraph::new(text)
        .block(
            Block::default()
                .title("Danger: Stop Selected Task")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.bad)),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(popup, area);
}

fn render_coordinator_recover_dialog(f: &mut Frame, state: &AppState) {
    let area = ui::centered_rect(65, 30, ui::content_area(f.size()));
    f.render_widget(Clear, area);
    let theme = ui::theme();

    let options = [
        "Recover: Apply full classification and save state changes.",
        "Recover (Dry Run): Classify tasks and print proposed action report without mutating.",
    ];

    let mut text = Vec::new();
    text.push(Line::from("Select recovery mode:"));
    text.push(Line::from(""));

    for (idx, opt) in options.iter().enumerate() {
        let style = if idx == state.coordinator_recover_dialog_selection {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
                .bg(theme.selection(navigation::mode(state)))
        } else {
            Style::default()
        };
        let prefix = if idx == state.coordinator_recover_dialog_selection {
            "> "
        } else {
            "  "
        };
        text.push(Line::from(vec![Span::styled(
            format!("{}{}", prefix, opt),
            style,
        )]));
    }

    text.push(Line::from(""));
    text.push(Line::from("Controls:"));
    text.push(Line::from("- ↑↓: Move selection"));
    text.push(Line::from("- Enter: Confirm and apply"));
    text.push(Line::from("- Esc or q/n: Cancel"));

    let popup = Paragraph::new(text)
        .block(
            Block::default()
                .title("Coordinator Recovery Modes")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent)),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(popup, area);
}

fn get_last_lines_of_file(path: &std::path::Path, limit: usize) -> Vec<String> {
    use std::io::{BufRead, BufReader};
    if let Ok(file) = std::fs::File::open(path) {
        let reader = BufReader::new(file);
        let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
        let start = lines.len().saturating_sub(limit);
        lines[start..].to_vec()
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod pause_overlay_content_tests {
    use super::pause_overlay_content;
    #[test]
    fn stale_context_is_recovery_not_an_unsatisfied_human_request() {
        let (title, body, _) = pause_overlay_content(
            "E903: DM_STALE: native decision binding changed",
            "task=VERIFY phase=fix",
            "run",
            Some("E903"),
            Some("VERIFY"),
            &["DM_STALE: native decision binding changed".into()],
            None,
        );
        assert_eq!(title, "Coordinator Paused — Context Refresh");
        assert!(body.contains("Work is preserved") && body.contains("checkpoint recovery"));
        assert!(!body.contains("cannot be implemented") && !body.contains("open Decisions"));
    }

    #[test]
    fn source_preconditions_offer_logs_and_compliance_objections_offer_decisions() {
        let source = "E903: out_of_scope_fix packages/ui-web/test/browser/interactions.spec.ts";
        let (_, body, _) = pause_overlay_content(
            source,
            "task=A phase=fix",
            "run",
            Some("E903"),
            Some("A"),
            &[source.into()],
            Some("Let the supervisor repair the protected source path."),
        );
        assert!(body.contains("open Logs") && !body.contains("open Decisions"));
        let finding="DM_COMPLIANCE: Decision choice v3: src/issuer.rs:26-41 conflicts with the normative transaction invariant";
        let (_, body, _) = pause_overlay_content(
            finding,
            "task=A phase=review",
            "run",
            Some("E903"),
            Some("A"),
            &[finding.into()],
            Some("Open Decisions; propose a governed revision of choice v3."),
        );
        assert!(
            body.contains("open Decisions")
                && body.contains("src/issuer.rs:26-41")
                && body.contains("choice v3")
        );
        assert!(!body.contains("external devices"));
    }

    #[test]
    fn precondition_stop_gets_its_own_title_and_lists_the_conditions() {
        let unmet = vec![
            "SEC-CR-001 approval pending".to_string(),
            "recovery-code entity undecided".to_string(),
        ];
        let (title, body, _) = pause_overlay_content(
            "Validation error: Coordinator cannot continue …",
            "task=SEC-API-003 phase=dev",
            "run",
            Some("E903"),
            Some("SEC-API-003"),
            &unmet,
            Some("Satisfy the listed preconditions."),
        );
        assert_eq!(title, "Coordinator Blocked — Preconditions Not Satisfied");
        assert!(body.starts_with(
            "SEC-API-003 cannot be implemented because the following preconditions are not satisfied:"
        ), "{body}");
        assert!(body.contains("  - SEC-CR-001 approval pending\n"));
        assert!(body.contains("  - recovery-code entity undecided\n"));
        assert!(body.contains("not a tool failure"));
        assert!(body.contains("Next: Satisfy the listed preconditions."));
        assert!(
            !body.contains("retry failed phase"),
            "retry menu must not be offered: {body}"
        );
        assert!(!body.contains("Coordinator Paused (blocking error)"));
    }

    #[test]
    fn precondition_stop_is_recognised_from_the_message_when_no_summary_is_loaded() {
        let (title, body, _) = pause_overlay_content(
            "Task X cannot be implemented because the following precondition is not satisfied:\n  - gate rejected [E903]",
            "global/blocking (no task context)",
            "run",
            None,
            None,
            &[],
            None,
        );
        assert_eq!(title, "Coordinator Blocked — Preconditions Not Satisfied");
        assert!(body.contains("gate rejected"), "{body}");
    }

    #[test]
    fn recorded_preconditions_select_the_precondition_overlay_for_any_error_code() {
        let unmet = vec!["allowed_paths must include apps/web/src/config.ts".to_string()];
        let (title, body, _) = pause_overlay_content(
            "Coordinator 'run' failed in 0:21:00.\n\nCause: # Performer log for task WEB-BFF-002",
            "task=WEB-BFF-002 phase=dev",
            "run",
            Some("E901"),
            Some("WEB-BFF-002"),
            &unmet,
            None,
        );
        assert_eq!(title, "Coordinator Blocked — Preconditions Not Satisfied");
        assert!(body.contains("  - allowed_paths must include apps/web/src/config.ts\n"));
        assert!(!body.contains("# Performer log"), "{body}");
    }

    #[test]
    fn diagnostic_delivery_waits_for_external_verification_without_retry_menu() {
        let (title, body, _) = pause_overlay_content(
            "DM_EXTERNAL_VERIFICATION: Diagnostic report complete; release not authorized",
            "task=VERIFY phase=review",
            "run",
            Some("E903"),
            Some("VERIFY"),
            &["Independent assistance observation pending".into()],
            Some("Verify the selected assistance or explicitly reconcile the decision."),
        );
        assert_eq!(title, "Coordinator Waiting — External Verification");
        assert!(body.contains("Independent assistance observation pending"));
        assert!(body.contains("explicitly reconcile"));
        assert!(!body.contains("retry failed phase"));
    }

    #[test]
    fn genuine_errors_keep_the_error_overlay() {
        let (title, body, _) = pause_overlay_content(
            "sandbox denied network",
            "task=T phase=dev",
            "run",
            Some("E902"),
            Some("T"),
            &[],
            None,
        );
        assert_eq!(title, "Coordinator Error");
        assert!(body.contains("retry failed phase"));
    }
}

#[cfg(test)]
mod approval_request_lines_tests {
    use super::approval_request_lines;
    use macc_core::coordinator::approval::PendingApproval;

    #[test]
    fn a_waiting_gate_shows_subject_approvers_and_the_command() {
        let lines = approval_request_lines(&[PendingApproval {
            task_id: "SEC-APP-003".into(),
            status: "waiting_approval".into(),
            subject_task: "SEC-ADR-003".into(),
            subject_revision: Some("abcdef1".into()),
            risks: vec!["Irreversible recovery-code migration".into()],
            required: "PRODUCT_OWNER 0/1, SECURITY_OWNER 0/1".into(),
            approve_command:
                "macc coordinator approve SEC-APP-003 --role PRODUCT_OWNER --revision abcdef1"
                    .into(),
            ..PendingApproval::default()
        }]);
        let text = lines
            .iter()
            .map(|(l, _)| l.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(lines[0].1, "headline is emphasised");
        assert!(text.contains("SEC-APP-003 waits for human approval"));
        assert!(text.contains("no performer runs"));
        assert!(text.contains("Subject:   SEC-ADR-003 at abcdef1"));
        assert!(text.contains("Risk:      Irreversible recovery-code migration"));
        assert!(text.contains("Approvers: PRODUCT_OWNER 0/1, SECURITY_OWNER 0/1"));
        assert!(text.contains("CLI:       macc coordinator approve SEC-APP-003"));
    }

    #[test]
    fn nothing_is_shown_when_no_gate_waits() {
        assert!(approval_request_lines(&[]).is_empty());
    }
}

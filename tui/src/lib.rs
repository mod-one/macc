use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use macc_core::service::coordinator_workflow::CoordinatorCommand;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap, Table, TableState, Row, Cell},
    Frame, Terminal,
};
use std::{collections::BTreeMap, io, time::Duration};

pub mod ownership;
pub mod screen;
pub mod state;
pub mod ui;

use macc_core::plan::{PlannedOpKind, Scope};
use macc_core::tool::{FieldDefault, FieldKind};
use screen::Screen;
use state::{AppState, UiStatusLevel};
use ui::{compact_help_line, header_lines, panel, theme, wrapped_paragraph, HeaderContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMode {
    Default,
    /// Launch directly into the coordinator live screen.
    /// `phase_overrides` is a human-readable summary of active runtime phase overrides
    /// (e.g. `"[testing:off] [review:required]"`), or `None` when none are active.
    CoordinatorRun { phase_overrides: Option<String> },
    /// Launch into the read-only observer/watch screen (`macc status --watch`).
    /// `control` enables operator actions (kill, stop, retry).
    /// `logs_only` collapses all panes except the log tail.
    /// `events_only` collapses all panes except the event timeline.
    Watch { control: bool, logs_only: bool, events_only: bool },
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
    match mode {
        LaunchMode::CoordinatorRun { phase_overrides } => {
            state.goto_screen(Screen::CoordinatorLive);
            state.start_coordinator_command(CoordinatorCommand::Run);
            state.coordinator_run_auto_quit = true;
            state.coordinator_phase_overrides = phase_overrides;
        }
        LaunchMode::Watch { control, logs_only, events_only } => {
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
    loop {
        state.tick();
        terminal.draw(|f| ui(f, state, true))?;

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(state, key.code);
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

fn handle_key(state: &mut AppState, key: KeyCode) {
    if state.coordinator_task_diff_popup.is_some() || state.coordinator_task_explain_popup.is_some() {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                state.coordinator_task_diff_popup = None;
                state.coordinator_task_explain_popup = None;
            }
            _ => {}
        }
        return;
    }

    if state.has_coordinator_pause_prompt() {
        match key {
            KeyCode::Char('r') | KeyCode::Enter => state.retry_after_coordinator_pause(),
            KeyCode::Char('s') => state.skip_after_coordinator_pause(),
            KeyCode::Char('o') => state.open_logs_after_coordinator_pause(),
            KeyCode::Char('u') => state.resume_signal_after_coordinator_pause(),
            KeyCode::Char('k') | KeyCode::Esc => state.stop_after_coordinator_pause(),
            KeyCode::Char('c') => state.resume_after_coordinator_pause(),
            _ => {}
        }
        return;
    }

    if state.help_open {
        match key {
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => {
                state.toggle_help();
                return;
            }
            _ => return,
        }
    }

    if state.search_editing {
        match key {
            KeyCode::Enter => state.commit_search(),
            KeyCode::Esc => state.cancel_search(),
            KeyCode::Backspace => state.pop_search_char(),
            KeyCode::Char(c) => state.append_search_char(c),
            _ => {}
        }
        return;
    }

    let current_screen = state.current_screen();
    let has_pending_takeover = current_screen == Screen::CoordinatorLive
        && state
            .coordinator_ownership
            .pending_incoming_request
            .is_some();

    if has_pending_takeover && matches!(key, KeyCode::Esc) {
        state.dismiss_takeover_request_modal();
        return;
    }

    if current_screen == Screen::ToolSettings && state.is_tool_field_editing() {
        match key {
            KeyCode::Enter => state.commit_tool_field_edit(),
            KeyCode::Esc => state.cancel_tool_field_edit(),
            KeyCode::Backspace => state.pop_tool_field_char(),
            KeyCode::Char(c) => state.append_tool_field_char(c),
            _ => {}
        }
        return;
    }
    // ── Field 3: Tool Priority reorder editor ───────────────────────────────
    if current_screen == Screen::Automation && state.tool_priority_editor_active {
        match key {
            // ↑ / k — navigate cursor up (when free), or move grabbed tool up
            KeyCode::Up | KeyCode::Char('k') => state.tool_priority_editor_up(),
            // ↓ / j — navigate cursor down (when free), or move grabbed tool down
            KeyCode::Down | KeyCode::Char('j') => state.tool_priority_editor_down(),
            // Space — grab / release the tool under the cursor for reordering
            KeyCode::Char(' ') => state.tool_priority_toggle_grab(),
            // Enter — release grab (drop in place) if grabbed; otherwise save and exit
            KeyCode::Enter => {
                if state.tool_priority_editor_grabbed {
                    state.tool_priority_toggle_grab();
                } else {
                    state.commit_tool_priority_editor();
                }
            }
            // s — save order and exit (regardless of grab state)
            KeyCode::Char('s') => state.commit_tool_priority_editor(),
            // Esc — first press releases grab, second press cancels editor
            KeyCode::Esc => state.cancel_tool_priority_editor(),
            _ => {}
        }
        return;
    }
    // ── Field 4: Max Parallel Per Tool editor ────────────────────────────────
    if current_screen == Screen::Automation && state.tool_parallel_editor_active {
        match key {
            // ↑ / k — navigate to previous tool
            KeyCode::Up | KeyCode::Char('k') => state.tool_parallel_editor_select(false),
            // ↓ / j — navigate to next tool
            KeyCode::Down | KeyCode::Char('j') => state.tool_parallel_editor_select(true),
            // ← / - — decrement count
            KeyCode::Left | KeyCode::Char('-') => state.tool_parallel_editor_adjust(-1),
            // → / + — increment count
            KeyCode::Right | KeyCode::Char('+') => state.tool_parallel_editor_adjust(1),
            // Enter / s — commit and exit
            KeyCode::Enter | KeyCode::Char('s') => state.cancel_tool_parallel_editor(),
            // Esc — cancel (changes already applied in-place; Ctrl+Z restores via snapshot)
            KeyCode::Esc => state.cancel_tool_parallel_editor(),
            _ => {}
        }
        return;
    }
    // ── Field 0: Coordinator Tool — left/right cycle while focused ──────────
    if current_screen == Screen::Automation
        && !state.is_automation_field_editing()
        && state.automation_field_index == 0
    {
        match key {
            KeyCode::Left => { state.cycle_coordinator_tool(false); return; }
            KeyCode::Right => { state.cycle_coordinator_tool(true); return; }
            _ => {}
        }
    }
    if current_screen == Screen::Automation && state.is_automation_field_editing() {
        match key {
            KeyCode::Enter => state.commit_automation_field_edit(),
            KeyCode::Esc => state.cancel_automation_field_edit(),
            KeyCode::Backspace => state.pop_automation_field_char(),
            KeyCode::Char(c) => state.append_automation_field_char(c),
            _ => {}
        }
        return;
    }
    if current_screen == Screen::Settings && state.is_settings_field_editing() {
        match key {
            KeyCode::Enter => state.commit_settings_field_edit(),
            KeyCode::Esc => state.cancel_settings_field_edit(),
            KeyCode::Backspace => state.pop_settings_field_char(),
            KeyCode::Char(c) => state.append_settings_field_char(c),
            _ => {}
        }
        return;
    }
    if current_screen == Screen::Tools && state.is_tool_install_confirmation_open() {
        match key {
            KeyCode::Char('y') | KeyCode::Enter => state.confirm_tool_install(),
            KeyCode::Char('n') | KeyCode::Esc => state.cancel_tool_install_confirmation(),
            _ => {}
        }
        return;
    }
    if current_screen == Screen::CoordinatorLive && state.coordinator_stop_dialog_open {
        match key {
            KeyCode::Up => {
                state.coordinator_stop_dialog_selection = state.coordinator_stop_dialog_selection.saturating_sub(1);
            }
            KeyCode::Down => {
                if state.coordinator_stop_dialog_selection < 3 {
                    state.coordinator_stop_dialog_selection += 1;
                }
            }
            KeyCode::Enter => {
                state.stop_coordinator_with_selected_mode();
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('n') => {
                state.close_coordinator_stop_dialog();
            }
            _ => {}
        }
        return;
    }
    if current_screen == Screen::CoordinatorLive && state.coordinator_recover_dialog_open {
        match key {
            KeyCode::Up => {
                state.coordinator_recover_dialog_selection = state.coordinator_recover_dialog_selection.saturating_sub(1);
            }
            KeyCode::Down => {
                if state.coordinator_recover_dialog_selection < 1 {
                    state.coordinator_recover_dialog_selection += 1;
                }
            }
            KeyCode::Enter => {
                state.recover_coordinator_with_selected_mode();
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('n') => {
                state.close_coordinator_recover_dialog();
            }
            _ => {}
        }
        return;
    }

    match key {
        KeyCode::Char('?') => {
            state.toggle_help();
        }
        KeyCode::Char('/') => {
            state.begin_search();
        }
        KeyCode::Enter => {
            if current_screen == Screen::CoordinatorLive {
                if let Some(task) = state.selected_live_task() {
                    let explain = state.get_task_explain(&task);
                    state.coordinator_task_explain_popup = Some(explain);
                }
            } else {
                state.navigate_enter();
            }
        }
        KeyCode::Backspace if current_screen == Screen::Apply => {
            state.pop_apply_consent_char();
        }
        KeyCode::Char(c) if current_screen == Screen::Apply && !matches!(c, 'q' | 'Q') => {
            state.append_apply_consent_char(c);
        }
        // Navigation: Esc/q to quit (or pop if in a sub-screen)
        KeyCode::Esc | KeyCode::Char('q') => {
            if state.screen_stack.len() > 1 {
                state.pop_screen();
            } else {
                state.should_quit = true;
            }
        }
        // Navigation: 'h' for Home, 't' for Tools
        KeyCode::Char('h') => state.goto_screen(Screen::Home),
        KeyCode::Char('t') => state.push_screen(Screen::Tools),
        KeyCode::Char('o') => state.push_screen(Screen::Automation),
        KeyCode::Char('v') if current_screen != Screen::CoordinatorLive => state.push_screen(Screen::CoordinatorLive),
        KeyCode::Char('m') => state.push_screen(Screen::Mcp),
        KeyCode::Char('g') => state.push_screen(Screen::Logs),
        KeyCode::Char('e') if current_screen != Screen::CoordinatorLive => state.push_screen(Screen::Settings),
        KeyCode::Char('p') => state.open_preview(),
        KeyCode::Char('x') if current_screen != Screen::Apply => {
            state.open_apply_screen();
        }

        // Navigation: Backspace to go back
        KeyCode::Backspace => state.pop_screen(),

        // Actions: 's' to Save Config
        KeyCode::Char('s') if current_screen != Screen::Apply && current_screen != Screen::CoordinatorLive => {
            state.save_config();
        }
        KeyCode::Char('u') if current_screen != Screen::CoordinatorLive => {
            state.undo_config_change();
        }
        KeyCode::Char('U') => {
            state.redo_config_change();
        }

        // Screen-specific controls
        KeyCode::Up => {
            state.navigate_prev();
        }
        KeyCode::Down => {
            state.navigate_next();
        }
        KeyCode::PageUp => {
            if current_screen == Screen::Preview {
                state.scroll_preview_diff(-10);
            } else if current_screen == Screen::Logs {
                state.scroll_log_content(-10);
            }
        }
        KeyCode::PageDown => {
            if current_screen == Screen::Preview {
                state.scroll_preview_diff(10);
            } else if current_screen == Screen::Logs {
                state.scroll_log_content(10);
            }
        }
        KeyCode::Char(' ') | KeyCode::Right => {
            state.navigate_toggle();
        }
        KeyCode::Char('a') => {
            if has_pending_takeover {
                state.ownership_respond_takeover(true);
            } else if current_screen == Screen::Skills {
                state.select_all_skills();
            } else if current_screen == Screen::Agents {
                state.select_all_agents();
            } else if current_screen == Screen::Mcp {
                state.select_all_mcp();
            } else if current_screen == Screen::Home {
                // Home screen: 'a' opens Apply screen.
                state.open_apply_screen();
            } else {
                state.push_screen(Screen::About);
            }
        }
        KeyCode::Char('n') => {
            if current_screen == Screen::Skills {
                state.select_no_skills();
            } else if current_screen == Screen::Agents {
                state.select_no_agents();
            } else if current_screen == Screen::Mcp {
                state.select_no_mcp();
            }
        }
        KeyCode::Char('r') => {
            if has_pending_takeover {
                state.ownership_respond_takeover(false);
            } else if current_screen == Screen::Home {
                // Home screen: 'r' starts the coordinator and navigates to Coordinator Live.
                state.start_coordinator_command(CoordinatorCommand::Run);
                state.push_screen(Screen::CoordinatorLive);
            } else if current_screen == Screen::Watch {
                // Force-refresh the RuntimeSnapshot immediately.
                state.refresh_watch_snapshot();
            } else if current_screen == Screen::CoordinatorLive {
                if let Some(task) = state.selected_live_task() {
                    match state.requeue_selected_task(task.task_id.clone()) {
                        Ok(_) => state.set_status(UiStatusLevel::Info, format!("Requeued task {}", task.task_id), Some(Duration::from_secs(3))),
                        Err(err) => state.set_status(UiStatusLevel::Error, format!("Failed to requeue: {}", err), Some(Duration::from_secs(4))),
                    }
                }
            } else if current_screen == Screen::Logs {
                state.refresh_logs();
            } else if current_screen == Screen::Preview {
                state.refresh_preview_plan();
            }
        }
        KeyCode::Char('R') if current_screen == Screen::CoordinatorLive => {
            if let Some(handle) = state.coordinator_handle() {
                state.try_owner_action(&handle, |state| {
                    state.start_coordinator_command(CoordinatorCommand::Run);
                });
            } else {
                state.start_coordinator_command(CoordinatorCommand::Run);
            }
        }
        KeyCode::Char('s') if current_screen == Screen::CoordinatorLive => {
            if let Some(task) = state.selected_live_task() {
                state.stop_selected_task(task.task_id.clone());
                state.set_status(UiStatusLevel::Info, format!("Sent kill request to task {}", task.task_id), Some(Duration::from_secs(3)));
            }
        }
        // Home screen: 'd' runs doctor check inline, shows results in Readiness panel.
        KeyCode::Char('d') if current_screen == Screen::Home => {
            state.run_home_doctor_check();
        }
        KeyCode::Char('d') if current_screen == Screen::CoordinatorLive => {
            if let Some(task) = state.selected_live_task() {
                let diff = state.get_task_diff(&task);
                state.coordinator_task_diff_popup = Some(diff);
            }
        }
        KeyCode::Char('e') if current_screen == Screen::CoordinatorLive => {
            if let Some(task) = state.selected_live_task() {
                let explain = state.get_task_explain(&task);
                state.coordinator_task_explain_popup = Some(explain);
            }
        }
        KeyCode::Char('f') if current_screen == Screen::CoordinatorLive => {
            state.begin_search();
        }
        KeyCode::Char('l') if current_screen == Screen::CoordinatorLive => {
            state.toggle_log_pane();
        }
        KeyCode::Char('y') if current_screen == Screen::CoordinatorLive => {
            if let Some(handle) = state.coordinator_handle() {
                state.try_owner_action(&handle, |state| {
                    state.start_coordinator_command(CoordinatorCommand::SyncRegistry);
                });
            } else {
                state.start_coordinator_command(CoordinatorCommand::SyncRegistry);
            }
        }
        KeyCode::Char('c') if current_screen == Screen::CoordinatorLive => {
            if let Some(handle) = state.coordinator_handle() {
                state.try_owner_action(&handle, |state| {
                    state.start_coordinator_command(CoordinatorCommand::ReconcileRuntime);
                });
            } else {
                state.start_coordinator_command(CoordinatorCommand::ReconcileRuntime);
            }
        }
        KeyCode::Char('u') if current_screen == Screen::CoordinatorLive => {
            if let Some(handle) = state.coordinator_handle() {
                state.try_owner_action(&handle, |state| {
                    state.start_coordinator_command(CoordinatorCommand::ResumePausedRun);
                });
            } else {
                state.start_coordinator_command(CoordinatorCommand::ResumePausedRun);
            }
        }
        KeyCode::Char('k') if current_screen == Screen::CoordinatorLive => {
            if let Some(handle) = state.coordinator_handle() {
                state.try_owner_action(&handle, |state| {
                    state.open_coordinator_stop_dialog();
                });
            } else {
                state.open_coordinator_stop_dialog();
            }
        }
        KeyCode::Char('v') if current_screen == Screen::CoordinatorLive => {
            if let Some(handle) = state.coordinator_handle() {
                state.try_owner_action(&handle, |state| {
                    state.open_coordinator_recover_dialog();
                });
            } else {
                state.open_coordinator_recover_dialog();
            }
        }
        KeyCode::Char('T')
            if current_screen == Screen::CoordinatorLive
                && !state.coordinator_ownership.is_owner =>
        {
            state.ownership_request_takeover();
        }
        KeyCode::Char('d') if current_screen == Screen::Tools => {
            state.refresh_tool_checks();
        }
        KeyCode::Char('i') if current_screen == Screen::Tools => {
            state.begin_tool_install_confirmation();
        }
        KeyCode::Char('f') if current_screen == Screen::Tools => {
            state.generate_context_for_selected_tool();
        }
        _ => {}
    }
}

fn ui(f: &mut Frame, state: &AppState, full_clear: bool) {
    let theme = theme();
    if full_clear {
        f.render_widget(Clear, f.size());
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // Title + status badges + trust strip + override strip
            Constraint::Min(0),    // Body
            Constraint::Length(4), // Footer / Navigation help
        ])
        .split(f.size());

    let current_screen = state.current_screen();

    // Header
    let project_label = state
        .project_paths
        .as_ref()
        .map(|p| p.root.display().to_string())
        .unwrap_or_else(|| "(no project)".to_string());
    let (config_label, config_color) = if state.working_copy.is_some() {
        ("loaded", theme.good)
    } else {
        ("missing", theme.warn)
    };
    let config_status = format!(
        "{} ({})",
        config_label,
        if config_color == theme.good {
            "ok"
        } else {
            "warn"
        }
    );
    let trust_strip = if let (Some(paths), Some(config)) = (&state.project_paths, state.working_copy.as_ref()) {
        let trust = macc_core::ops_motif::calculate_trust_summary(paths, config);
        let local_only = if trust.local_only { "yes" } else { "no" };
        let terminal = if trust.terminal_enabled { "enabled" } else { "disabled" };
        let backups = if trust.backups_ready { "ready" } else { "missing" };
        let catalog = if trust.catalog_pinned { "pinned" } else { "unpinned" };
        let secrets = if trust.secrets_redacted { "redacted" } else { "unredacted" };
        Some(format!(
            "Local only: {} | Terminal: {} | User writes: {} | Backups: {} | Catalog: {} | Secrets: {}",
            local_only, terminal, trust.user_level_writes, backups, catalog, secrets
        ))
    } else {
        None
    };
    let header_ctx = HeaderContext {
        app_name: "[M][A][C][C]",
        screen_title: current_screen.title(),
        mode: state.interaction_mode_label(),
        project: &project_label,
        config_label: &config_status,
        errors: state.errors.len(),
        coordinator_active: state.is_coordinator_running(),
        coordinator_paused: state.is_coordinator_paused(),
        coordinator_command: state.coordinator_running_command.as_deref(),
        status: state.status_line(),
        width: chunks[0].width,
        trust_strip,
        override_strip: state.coordinator_phase_overrides.clone(),
    };
    let title = Paragraph::new(header_lines(&header_ctx, &theme)).block(panel("MACC"));
    f.render_widget(title, chunks[0]);

    // Body
    match current_screen {
        Screen::Skills => {
            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[1]);
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
                    let is_required = macc_core::is_required_skill(&skill.id);
                    let enabled_marker = if is_enabled { "[x]" } else { "[ ]" };
                    let required_badge = if is_required { " [required]" } else { "" };
                    ListItem::new(Line::from(vec![
                        Span::raw(enabled_marker),
                        Span::raw(" "),
                        Span::raw(format!("{}{}", skill.name, required_badge)),
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
                .highlight_style(Style::default().bg(theme.highlight_bg));
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
                if macc_core::is_required_skill(&current_skill.id) {
                    desc_text.push_str("\n\nRequired skill: always enabled (read-only toggle).");
                }
                desc_text.push_str("\n\n---\nShortcuts:\n'a' - Select All\n'n' - Select None");

                let desc_para = Paragraph::new(desc_text).block(panel("Details"));
                f.render_widget(desc_para, body_chunks[1]);
            }
        }
        Screen::Mcp => {
            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[1]);
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
                .highlight_style(Style::default().bg(theme.highlight_bg));
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
                detail.push_str("\n\nNotes:\n- MCP packages are merged into .mcp.json on apply.\n- Secrets are never stored by MACC.\n\nShortcuts:\n'a' - Select All\n'n' - Select None");

                let desc_para = Paragraph::new(detail).block(panel("Details"));
                f.render_widget(desc_para, body_chunks[1]);
            }
        }
        Screen::Logs => {
            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
                .split(chunks[1]);

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
                .highlight_style(Style::default().bg(theme.highlight_bg));
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
                .split(chunks[1]);

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
                .highlight_style(Style::default().bg(theme.highlight_bg));
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
                desc_text.push_str("\n\n---\nShortcuts:\n'a' - Select All\n'n' - Select None");

                let desc_para = Paragraph::new(desc_text).block(panel("Details"));
                f.render_widget(desc_para, body_chunks[1]);
            }
        }
        Screen::Preview => {
            let mut preview_constraints = Vec::new();
            if state.preview_error.is_some() {
                preview_constraints.push(Constraint::Length(3));
            }
            preview_constraints.push(Constraint::Length(4));
            preview_constraints.push(Constraint::Min(0));

            let preview_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(&preview_constraints)
                .split(chunks[1]);

            let mut chunk_index = 0;
            if let Some(error) = &state.preview_error {
                let error_para = Paragraph::new(error.clone())
                    .style(Style::default().fg(Color::Red))
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
            summary_text.push_str(
                "\nPress 'x' to open Apply (consent required for any user-level operations).",
            );

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
                .block(panel("Planned Operations"))
                .highlight_symbol("▶ ")
                .highlight_style(Style::default().fg(Color::Yellow));
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
                let notice = Paragraph::new(
                    "Large diff truncated; scroll with PageUp/PageDown to view more.",
                )
                .style(Style::default().fg(Color::Yellow))
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
        Screen::Apply => {
            let apply_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(20),
                    Constraint::Percentage(40),
                    Constraint::Percentage(15),
                    Constraint::Percentage(25),
                ])
                .split(chunks[1]);

            let summary_rect = apply_chunks[0];
            let operations_rect = apply_chunks[1];
            let consent_rect = apply_chunks[2];
            let result_rect = apply_chunks[3];

            let progress_text = if let Some(progress) = &state.apply_progress {
                let path = progress.path.as_deref().unwrap_or("(pending)");
                format!(
                    "Progress: {}/{} operations (current: {})",
                    progress.current, progress.total, path
                )
            } else {
                "Progress: not started".to_string()
            };

            let summary_base = if let Some(ctx) = &state.apply_context {
                let consent_required = if ctx.needs_user_consent() {
                    "yes"
                } else {
                    "no"
                };
                format!(
                    "Total operations: {}\nProject: {}\nUser: {}\nBackup preview: {}\nUser consent required: {}",
                    ctx.operations.len(),
                    ctx.project_ops,
                    ctx.user_ops,
                    ctx.backup_preview,
                    consent_required
                )
            } else {
                state
                    .apply_error
                    .as_ref()
                    .map(|e| format!("Unable to compute apply plan:\n{}", e))
                    .unwrap_or_else(|| {
                        "Plan unavailable. Try refreshing the preview first.".to_string()
                    })
            };

            let apply_stage = if state.apply_error.is_some() {
                "failed"
            } else if state.apply_feedback.is_some() {
                "done"
            } else if state.apply_progress.is_some() {
                "running"
            } else {
                "ready"
            };
            let summary_text = format!(
                "Pipeline: plan -> consent -> apply -> verify\nStage: {}\n\n{}\n{}",
                apply_stage, summary_base, progress_text
            );

            let summary_para = Paragraph::new(summary_text)
                .block(panel("Apply Summary"))
                .wrap(Wrap { trim: false });
            f.render_widget(summary_para, summary_rect);

            let operations_text = if let Some(ctx) = &state.apply_context {
                if ctx.operations.is_empty() {
                    "No operations planned.".to_string()
                } else {
                    let mut lines: Vec<String> = ctx
                        .operations
                        .iter()
                        .take(10)
                        .map(|op| {
                            format!(
                                "[{}] {:<8} {}",
                                scope_label(op.scope),
                                kind_label(op.kind),
                                op.path
                            )
                        })
                        .collect();
                    if ctx.operations.len() > 10 {
                        lines.push(format!(
                            "...and {} more operations",
                            ctx.operations.len() - 10
                        ));
                    }
                    lines.join("\n")
                }
            } else {
                "Preview the plan first to see operation details.".to_string()
            };

            let operations_para = Paragraph::new(operations_text)
                .block(panel("Operation Snapshot"))
                .wrap(Wrap { trim: false });
            f.render_widget(operations_para, operations_rect);

            let consent_prompt = if let Some(ctx) = &state.apply_context {
                if ctx.needs_user_consent() {
                    "Type YES (case-insensitive) below to allow user-scope operations."
                } else {
                    "No user-scope operations detected; press Enter to apply the project-only changes."
                }
            } else {
                "Consent state unavailable until a plan is computed."
            };
            let consent_status = if state.apply_user_consent_granted {
                "Consent confirmed"
            } else {
                "Consent pending"
            };
            let consent_input = if state.apply_consent_input.is_empty() {
                "<empty>".to_string()
            } else {
                state.apply_consent_input.clone()
            };
            let consent_text = format!(
                "{}\nInput buffer: {}\n{}",
                consent_status, consent_input, consent_prompt
            );
            let consent_para = Paragraph::new(consent_text)
                .block(panel("User Consent"))
                .wrap(Wrap { trim: false });
            f.render_widget(consent_para, consent_rect);

            let result_text = if let Some(err) = &state.apply_error {
                format!("Error applying changes:\n{}", err)
            } else if let Some(feedback) = &state.apply_feedback {
                feedback.clone()
            } else {
                "Awaiting apply. Press Enter to run once consent requirements (if any) are satisfied."
                    .to_string()
            };
            let result_style = if state.apply_error.is_some() {
                Style::default().fg(Color::Red)
            } else if state.apply_feedback.is_some() {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };
            let result_para = Paragraph::new(result_text)
                .style(result_style)
                .block(panel("Apply Result"))
                .wrap(Wrap { trim: false });
            f.render_widget(result_para, result_rect);
        }
        Screen::Home => {
            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(chunks[1]);

            let mut overview = String::new();
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
                    overview.push_str(&format!("Worktrees Total: {}\n", status.total));
                } else {
                    overview.push_str(&format!(
                        "Worktree: (none) | Worktrees Total: {}\n",
                        status.total
                    ));
                }
            }

            let overview_para = wrapped_paragraph(overview, "Overview");
            f.render_widget(overview_para, body_chunks[0]);

            // Readiness ladder (spec §9 / §13.1) — via Engine facade.
            let readiness_text = if let Some(paths) = &state.project_paths {
                let ladder = state.engine.readiness_ladder(paths);
                build_readiness_text(&ladder, state.home_doctor_summary.as_deref())
            } else {
                "Run 'macc init' to set up this project.\n\nActions:\n  [d] Doctor check\n  CLI: macc quickstart".to_string()
            };
            let steps_para = wrapped_paragraph(readiness_text, "Readiness");
            f.render_widget(steps_para, body_chunks[1]);
        }
        Screen::Settings => {
            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(chunks[1]);

            let mut list_state = ListState::default();
            let settings_count = state.settings_field_count();
            list_state.select(Some(
                state
                    .settings_field_index
                    .min(settings_count.saturating_sub(1)),
            ));
            let items: Vec<ListItem> = (0..settings_count)
                .map(|i| {
                    let label = state.settings_field_label(i);
                    let value =
                        if i == state.settings_field_index && state.is_settings_field_editing() {
                            format!("{}_", state.settings_field_input)
                        } else {
                            state.settings_field_display_value(i)
                        };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:<30}", label), Style::default().fg(theme.muted)),
                        Span::raw(" "),
                        Span::raw(value),
                    ]))
                })
                .collect();

            let list = List::new(items)
                .block(panel("Global Settings"))
                .highlight_symbol("› ")
                .highlight_style(Style::default().bg(theme.highlight_bg));
            f.render_stateful_widget(list, body_chunks[0], &mut list_state);

            let help = state.settings_field_help(state.settings_field_index);
            let help_para = Paragraph::new(help)
                .block(panel("Setting Description"))
                .wrap(Wrap { trim: true });
            f.render_widget(help_para, body_chunks[1]);
        }
        Screen::Automation => {
            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(chunks[1]);

            let mut list_state = ListState::default();
            let automation_count = state.automation_field_count();
            list_state.select(Some(
                state
                    .automation_field_index
                    .min(automation_count.saturating_sub(1)),
            ));
            let items: Vec<ListItem> = (0..automation_count)
                .map(|i| {
                    let label = state.automation_field_label(i);
                    let value = if i == state.automation_field_index
                        && state.is_automation_field_editing()
                    {
                        format!("{}_", state.automation_field_input)
                    } else if i == 3 && state.tool_priority_editor_active {
                        if state.tool_priority_editor_grabbed {
                            "[GRABBED — ↑↓ to move, Space to drop]".to_string()
                        } else {
                            "[reorder mode — ↑↓ navigate, Space to grab]".to_string()
                        }
                    } else if i == 4 && state.tool_parallel_editor_active {
                        "[edit mode — ↑↓ select, ←→ adjust, Enter done]".to_string()
                    } else {
                        state.automation_field_display_value(i)
                    };
                    // §18/§19: highlight phase fields in warning colour when a CLI override is active
                    let has_override = state.phase_override_notice_for_field(i).is_some();
                    let value_style = if has_override {
                        Style::default().fg(theme.warn)
                    } else if (i == 3 && state.tool_priority_editor_active)
                        || (i == 4 && state.tool_parallel_editor_active)
                    {
                        Style::default().fg(theme.accent)
                    } else {
                        Style::default()
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:<30}", label), Style::default().fg(theme.muted)),
                        Span::raw(" "),
                        Span::styled(value, value_style),
                    ]))
                })
                .collect();

            let list = List::new(items)
                .block(panel("Coordinator Settings"))
                .highlight_symbol("› ")
                .highlight_style(Style::default().bg(theme.highlight_bg));
            f.render_stateful_widget(list, body_chunks[0], &mut list_state);

            let idx = state
                .automation_field_index
                .min(automation_count.saturating_sub(1));
            // §18/§19: prepend a CLI override warning for phase fields when an override is active
            let override_prefix = state
                .phase_override_notice_for_field(idx)
                .map(|notice| format!("⚠ {notice}\n\n"))
                .unwrap_or_default();
            // ── Special editor detail panes ──────────────────────────────────
            let detail_para = if state.tool_priority_editor_active {
                // Field 3: priority reorder editor — show ordered list in detail pane
                let list = state.tool_priority_ordered_list();
                let sel = state.tool_priority_editor_index;
                let grabbed = state.tool_priority_editor_grabbed;

                let title = if grabbed {
                    "Tool Priority — GRABBED (moving)"
                } else {
                    "Tool Priority — Select & Reorder"
                };

                let mut lines: Vec<String> = if grabbed {
                    vec![
                        "A tool is grabbed — it will move with ↑/↓.".to_string(),
                        String::new(),
                        "↑/k  Move tool up (higher priority)".to_string(),
                        "↓/j  Move tool down (lower priority)".to_string(),
                        "Space/Enter  Drop here (release grab)".to_string(),
                        "s    Save order and exit".to_string(),
                        "Esc  Release grab (do not exit)".to_string(),
                    ]
                } else {
                    vec![
                        "Navigate with ↑/↓, then Space to grab.".to_string(),
                        String::new(),
                        "↑/k  Move cursor up".to_string(),
                        "↓/j  Move cursor down".to_string(),
                        "Space  Grab selected tool for moving".to_string(),
                        "Enter/s  Save order and exit".to_string(),
                        "Esc  Cancel (discard changes)".to_string(),
                    ]
                };
                lines.push(String::new());

                for (i, tool) in list.iter().enumerate() {
                    let cursor = if i == sel {
                        if grabbed { "✦ " } else { "› " }
                    } else {
                        "  "
                    };
                    lines.push(format!("{}{}. {}", cursor, i + 1, tool));
                }
                wrapped_paragraph(lines.join("\n"), title)
            } else if state.tool_parallel_editor_active {
                // Field 4: per-tool parallel count editor
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
                    "Max Parallel Per Tool — Edit Mode".to_string(),
                    String::new(),
                    "↑/k    Select previous tool".to_string(),
                    "↓/j    Select next tool".to_string(),
                    "←/−    Decrease count (min 1)".to_string(),
                    "→/+    Increase count".to_string(),
                    "Enter/s  Exit editor".to_string(),
                    String::new(),
                ];
                for (i, tool) in enabled.iter().enumerate() {
                    let count = counts.get(tool).copied().unwrap_or(1);
                    let marker = if i == sel { "› " } else { "  " };
                    lines.push(format!("{}{}: {}", marker, tool, count));
                }
                wrapped_paragraph(lines.join("\n"), "Max Parallel Per Tool Editor")
            } else if idx == 0 {
                // Field 0: coordinator tool — show available options
                let enabled = state
                    .working_copy
                    .as_ref()
                    .map(|wc| wc.tools.enabled.clone())
                    .unwrap_or_default();
                let current = state.automation_field_display_value(0);
                let mut lines = vec![
                    format!("{}Field: {}", override_prefix, state.automation_field_label(0)),
                    String::new(),
                    state.automation_field_help(0).to_string(),
                    String::new(),
                    format!("Current: {}", if current.is_empty() { "(auto-select)" } else { &current }),
                    String::new(),
                    "Available tools:".to_string(),
                    "  (empty)  — auto-select".to_string(),
                ];
                for t in &enabled {
                    let marker = if t == &current { " ✓ " } else { "   " };
                    lines.push(format!("{}{}", marker, t));
                }
                lines.push(String::new());
                lines.push("Space/Enter or ←/→ to cycle".to_string());
                lines.push("s — Save to .macc/macc.yaml".to_string());
                wrapped_paragraph(lines.join("\n"), "Field Info")
            } else {
                // Default detail pane for all other fields
                let mut detail = format!(
                    "{}Field: {}\n\n{}\n\nSaved config: {}\n\nShortcuts:\nSpace/Enter - Edit or cycle\nEsc - Cancel edit\ns - Save to .macc/macc.yaml",
                    override_prefix,
                    state.automation_field_label(idx),
                    state.automation_field_help(idx),
                    state.automation_field_display_value(idx),
                );
                if let Some(validation) = state.current_automation_field_validation() {
                    detail.push_str(&format!("\n\nValidation:\n{}", validation));
                }
                detail.push_str("\n\nRuntime monitoring moved to Coordinator Live.\nPress 'v' to open live status, active tasks, and events.");
                wrapped_paragraph(detail, "Field Info")
            };
            f.render_widget(detail_para, body_chunks[1]);
        }
        Screen::CoordinatorLive => {
            // L6-TUI-003: ownership banner row above the main split.
            let live_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // ownership banner
                    Constraint::Length(1), // summary header status line
                    Constraint::Min(0),    // vertical stacked panes
                ])
                .split(chunks[1]);
            render_coordinator_ownership_banner(f, live_chunks[0], state);

            let status_line = if state.is_coordinator_paused() {
                "PAUSED (awaiting resume)".to_string()
            } else if state.is_coordinator_running() {
                format!(
                    "Running: {} ({}) {}",
                    state
                        .coordinator_running_command
                        .as_deref()
                        .unwrap_or("unknown"),
                    format_hms(state.coordinator_elapsed_seconds().unwrap_or(0)),
                    state.coordinator_spinner_frame()
                )
            } else {
                "Idle".to_string()
            };
            let snapshot_line = if let Some(s) = &state.coordinator_snapshot {
                format!(
                    "total={} todo={} active={} blocked={} merged={}",
                    s.total, s.todo, s.active, s.blocked, s.merged
                )
            } else {
                "unavailable".to_string()
            };
            let search_line = if !state.search_query.is_empty() {
                format!(" | Search: '{}'", state.search_query)
            } else {
                String::new()
            };
            let summary_para = Paragraph::new(format!(
                "Coordinator: {} | Tasks: {}{}",
                status_line, snapshot_line, search_line
            )).style(Style::default().fg(theme.accent_dim));
            f.render_widget(summary_para, live_chunks[1]);

            // Layout the 3 vertical stacked panes: LIVE TASKS table + DETAIL + LIVE LOGS (optional)
            let body_constraints = if state.coordinator_log_pane_visible {
                vec![
                    Constraint::Percentage(45),
                    Constraint::Percentage(25),
                    Constraint::Percentage(30),
                ]
            } else {
                vec![
                    Constraint::Percentage(65),
                    Constraint::Percentage(35),
                ]
            };

            let body_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(body_constraints)
                .split(live_chunks[2]);

            // Pane 1: LIVE TASKS Table
            let filtered_tasks = state.filtered_active_tasks();
            let mut rows = Vec::new();
            for task in &filtered_tasks {
                let health_symbol = task.health.symbol();

                let health_style = match task.health {
                    macc_core::coordinator::view_model::TaskHealth::Warning => Style::default().fg(theme.bad),
                    macc_core::coordinator::view_model::TaskHealth::Stale => Style::default().fg(theme.warn),
                    macc_core::coordinator::view_model::TaskHealth::Healthy => Style::default().fg(theme.good),
                    _ => Style::default().fg(theme.muted),
                };

                let status_label = task.status_label();

                let phase_label = task.phase.compact_label();

                let status_text = if phase_label.is_empty() {
                    status_label
                } else {
                    format!("{} {}", status_label, phase_label)
                };

                let age_label = task.age_label();
                let hb_label = task.heartbeat_age_label();

                let worker = if task.worker_id.is_empty() { "-" } else { &task.worker_id };
                let tool = if task.tool.is_empty() { "-" } else { &task.tool };

                let cells = vec![
                    Cell::from(health_symbol).style(health_style),
                    Cell::from(worker.to_string()),
                    Cell::from(task.task_id.clone()),
                    Cell::from(status_text),
                    Cell::from(tool.to_string()),
                    Cell::from(age_label),
                    Cell::from(hb_label),
                ];
                rows.push(Row::new(cells));
            }

            let headers = Row::new(vec![
                Cell::from("Health").style(Style::default().fg(theme.accent)),
                Cell::from("Worker").style(Style::default().fg(theme.accent)),
                Cell::from("Task ID").style(Style::default().fg(theme.accent)),
                Cell::from("Status").style(Style::default().fg(theme.accent)),
                Cell::from("Tool").style(Style::default().fg(theme.accent)),
                Cell::from("Age").style(Style::default().fg(theme.accent)),
                Cell::from("HB").style(Style::default().fg(theme.accent)),
            ]);

            let widths = [
                Constraint::Length(8),
                Constraint::Length(12),
                Constraint::Length(25),
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Length(8),
                Constraint::Length(8),
            ];
            let tasks_table = Table::new(rows, widths)
                .header(headers)
                .block(panel("LIVE TASKS (↑↓ navigate, Enter details, d diff, r retry, s stop task, k stop coordinator)"))
                .highlight_style(Style::default().bg(theme.highlight_bg))
                .highlight_symbol("› ");

            let mut table_state = TableState::default();
            if !filtered_tasks.is_empty() {
                let clamped_idx = state.coordinator_selected_task_index.min(filtered_tasks.len() - 1);
                table_state.select(Some(clamped_idx));
            }
            f.render_stateful_widget(tasks_table, body_chunks[0], &mut table_state);

            // Pane 2: SELECTED TASK DETAIL
            let selected_task = state.selected_live_task();
            let mut detail_lines = Vec::new();
            if let Some(ref t) = selected_task {
                let full_task = state.load_coordinator_storage_snapshot().ok().and_then(|s| s.registry.tasks.into_iter().find(|rt| rt.id == t.task_id));
                let title = full_task.as_ref().and_then(|rt| rt.title.clone()).unwrap_or_else(|| "(no title)".to_string());
                let worktree = full_task.as_ref().and_then(|rt| rt.task_runtime.worktree.clone()).unwrap_or_else(|| t.worktree.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "-".to_string()));
                let branch = full_task.as_ref().and_then(|rt| rt.task_runtime.branch.clone()).unwrap_or_else(|| t.branch.clone().unwrap_or_else(|| "-".to_string()));
                
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
                    Span::styled(format!("{} (phase: {})", t.runtime_status.as_str(), t.phase.compact_label()), Style::default()),
                    Span::styled("   Tool: ", Style::default().fg(theme.muted)),
                    Span::styled(&t.tool, Style::default()),
                ]));
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
            } else {
                detail_lines.push(Line::from("No task selected. Use ↑/↓ to navigate."));
            }
            let detail_para = Paragraph::new(detail_lines)
                .block(panel("SELECTED TASK DETAIL"))
                .wrap(Wrap { trim: true });
            f.render_widget(detail_para, body_chunks[1]);

            // Pane 3: LIVE LOGS timeline (optional)
            if state.coordinator_log_pane_visible {
                let mut logs_lines = Vec::new();
                if let Some(ref t) = selected_task {
                    let full_task = state.load_coordinator_storage_snapshot().ok().and_then(|s| s.registry.tasks.into_iter().find(|rt| rt.id == t.task_id));
                    let mut read_success = false;
                    if let Some(ref ft) = full_task {
                        if let Some(ref stdout_rel) = ft.task_runtime.stdout_log {
                            if let Some(ref paths) = state.project_paths {
                                let path = paths.root.join(stdout_rel);
                                if path.exists() {
                                    let lines = get_last_lines_of_file(&path, 15);
                                    if !lines.is_empty() {
                                        logs_lines.push(Line::from(vec![
                                            Span::styled(format!("Source: stdout ({})", stdout_rel), Style::default().fg(theme.accent_dim)),
                                        ]));
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
                        logs_lines.push(Line::from("No active stdout log file found. Showing matching coordinator events:"));
                        for line in state.coordinator_events.iter().rev() {
                            if line.contains(&t.task_id) {
                                logs_lines.push(Line::from(line.clone()));
                            }
                        }
                    }
                } else {
                    logs_lines.push(Line::from("No task selected. Showing recent coordinator events:"));
                    for line in state.coordinator_events.iter().rev().take(15).rev() {
                        logs_lines.push(Line::from(line.clone()));
                    }
                }
                let logs_para = Paragraph::new(logs_lines)
                    .block(panel("LIVE LOGS"))
                    .wrap(Wrap { trim: true });
                f.render_widget(logs_para, body_chunks[2]);
            }

            // Render popups if open
            if let Some(ref diff) = state.coordinator_task_diff_popup {
                let area = ui::centered_rect(85, 85, f.size());
                let diff_para = Paragraph::new(diff.as_str())
                    .block(panel("Task Diff (Press Esc/q to Close)"))
                    .wrap(Wrap { trim: false });
                f.render_widget(Clear, area);
                f.render_widget(diff_para, area);
            }
            if let Some(ref explain) = state.coordinator_task_explain_popup {
                let area = ui::centered_rect(85, 85, f.size());
                let explain_para = Paragraph::new(explain.as_str())
                    .block(panel("Task Explanation & Timeline (Press Esc/q to Close)"))
                    .wrap(Wrap { trim: false });
                f.render_widget(Clear, area);
                f.render_widget(explain_para, area);
            }
        }
        Screen::Tools => {
            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(chunks[1]);

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

            let items: Vec<ListItem> = state
                .filtered_tool_indices()
                .iter()
                .map(|index| {
                    let tool = &state.tool_descriptors[*index];
                    let is_enabled = enabled_tools.contains(&tool.id.to_string());
                    let enabled_marker = if is_enabled { "[x]" } else { "[ ]" };
                    let status = state
                        .tool_checks
                        .iter()
                        .find(|tc| tc.tool_id.as_deref() == Some(tool.id.as_str()))
                        .map(|tc| tc.status.clone())
                        .unwrap_or(macc_core::doctor::ToolStatus::Missing);
                    let (status_label, status_color) = match status {
                        macc_core::doctor::ToolStatus::Installed => ("installed", theme.good),
                        macc_core::doctor::ToolStatus::Missing => ("missing", theme.warn),
                        macc_core::doctor::ToolStatus::Error(_) => ("error", theme.bad),
                    };
                    let install_hint = if matches!(status, macc_core::doctor::ToolStatus::Missing)
                        && tool.install.is_some()
                    {
                        " install"
                    } else {
                        ""
                    };

                    ListItem::new(Line::from(vec![
                        Span::raw(enabled_marker),
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
                .block(panel(&title))
                .highlight_symbol("› ")
                .highlight_style(Style::default().bg(theme.highlight_bg));
            f.render_stateful_widget(list, body_chunks[0], &mut list_state);

            if visible.is_empty() {
                f.render_widget(
                    Paragraph::new("No matching tools. Press '/' to edit search.")
                        .block(Block::default().borders(Borders::ALL)),
                    body_chunks[1],
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
                    .map(|tc| tc.status.clone())
                    .unwrap_or(macc_core::doctor::ToolStatus::Missing);
                let status_label = match &status {
                    macc_core::doctor::ToolStatus::Installed => "installed",
                    macc_core::doctor::ToolStatus::Missing => "missing",
                    macc_core::doctor::ToolStatus::Error(_) => "error",
                };
                let mut detail = format!(
                    "ID: {}\nStatus: {}\nFields: {}\n\nDescription:\n{}\n",
                    tool.id,
                    status_label,
                    tool.fields.len(),
                    tool.description
                );
                if let Some(install) = &tool.install {
                    detail.push_str("\nInstall:\n");
                    detail.push_str(&install.confirm_message);
                }
                if let macc_core::doctor::ToolStatus::Error(msg) = status {
                    detail.push_str("\nError:\n");
                    detail.push_str(&msg);
                }
                detail.push_str("\n\nShortcuts:\nSpace - Toggle\nEnter - Configure\n'i' - Install missing tool\n'd' - Refresh checks\n'f' - Generate context file");
                if state.is_tool_install_confirmation_open() {
                    detail.push_str(
                        "\n\nInstall confirmation pending: press 'y' to install, 'n' to cancel.",
                    );
                }

                let detail_para = Paragraph::new(detail)
                    .block(panel("Details"))
                    .wrap(Wrap { trim: false });
                f.render_widget(detail_para, body_chunks[1]);
            }
        }
        Screen::ToolSettings => {
            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(chunks[1]);

            if let Some(desc) = state.current_tool_descriptor() {
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
                        let value = if is_selected && state.is_tool_field_editing() {
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
                    .block(panel(&settings_title))
                    .highlight_symbol("› ")
                    .highlight_style(Style::default().bg(theme.highlight_bg));
                f.render_stateful_widget(list, body_chunks[0], &mut list_state);

                let mut detail = String::new();
                if let Some(field) = state.current_tool_field() {
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
                detail.push_str("\n\nShortcuts:\nSpace/Enter - Edit\nEsc - Cancel edit");

                if let Some(validation) = state.current_tool_field_validation() {
                    detail.push_str("\n\nValidation:\n");
                    detail.push_str(&validation);
                }
                let detail_para = wrapped_paragraph(detail, "Field Info");
                f.render_widget(detail_para, body_chunks[1]);
            } else {
                let body = wrapped_paragraph("No tool selected. Return to Tools.", "Tool Settings");
                f.render_widget(body, chunks[1]);
            }
        }
        Screen::About => {
            let body = wrapped_paragraph(
                "About MACC\n\nThis is the v0.2 prototype.\n\nUse Backspace or Esc to go back.",
                "About",
            );
            f.render_widget(body, chunks[1]);
        }
        Screen::Watch => {
            render_watch_screen(f, state, chunks[1]);
        }
    }

    // Footer
    let badges = state.status_badges().join(" | ");
    let search = if state.search_editing {
        format!("search> {}_", state.search_query)
    } else if !state.search_query.is_empty() {
        format!("search: {}", state.search_query)
    } else {
        "search: (off)".to_string()
    };
    let footer = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Path: ", Style::default().fg(theme.muted)),
            Span::raw(ui::truncate_middle(
                &state.breadcrumbs(),
                chunks[2].width.saturating_sub(8) as usize,
            )),
        ]),
        Line::from(vec![
            Span::styled("State: ", Style::default().fg(theme.muted)),
            Span::raw(ui::truncate_middle(
                &format!("{} | {}", badges, search),
                chunks[2].width.saturating_sub(9) as usize,
            )),
        ]),
        footer_hints_line(state, &theme, chunks[2].width.saturating_sub(20) as usize),
    ])
    .block(panel("Navigation"));
    f.render_widget(footer, chunks[2]);

    if state.has_coordinator_pause_prompt() {
        render_coordinator_pause_overlay(f, state);
    }
    if let Some(req) = state
        .coordinator_ownership
        .pending_incoming_request
        .as_ref()
    {
        if state.current_screen() == Screen::CoordinatorLive {
            crate::ownership::render_takeover_modal(f, f.size(), req);
        }
    }
    if state.current_screen() == Screen::CoordinatorLive {
        if state.coordinator_stop_dialog_open {
            render_coordinator_stop_dialog(f, state);
        }
        if state.coordinator_recover_dialog_open {
            render_coordinator_recover_dialog(f, state);
        }
    }
    if state.help_open {
        render_help_overlay(f, state);
    }
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
        Some(FieldDefault::Bool(value)) => Some(value.to_string()),
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

fn build_readiness_text(
    ladder: &macc_core::onboarding::ReadinessLadder,
    doctor_summary: Option<&str>,
) -> String {
    // If a doctor check has been run, show its detailed output instead of the ladder.
    if let Some(summary) = doctor_summary {
        let mut out = summary.to_string();
        out.push_str("\nActions: [d] re-check  [r] start coordinator  [a] apply  [v] live view");
        return out;
    }

    let mut out = String::new();
    out.push_str("MACC readiness\n\n");
    for step in &ladder.steps {
        let symbol = step.symbol();
        let detail = step.detail.as_deref().unwrap_or("");
        if detail.is_empty() {
            out.push_str(&format!("{}. {}  {}\n", step.number, step.label, symbol));
        } else {
            out.push_str(&format!(
                "{}. {}  {}  {}\n",
                step.number, step.label, symbol, detail
            ));
        }
    }
    out.push('\n');
    if ladder.is_ready() {
        out.push_str("✅ Ready to dispatch a task\n\n");
        out.push_str("Actions: [r] start coordinator  [v] live view  [d] doctor check");
    } else {
        out.push_str(&format!(
            "❌ {} step(s) pending\n\n",
            ladder.blocking_count
        ));
        out.push_str("Actions:\n");
        out.push_str("  [d] Doctor check\n");
        out.push_str("  [a] Apply config\n");
        out.push_str("  [r] Start coordinator\n");
        out.push_str("  [v] Coordinator live view\n");
        out.push_str("  CLI: macc quickstart");
    }
    out
}

fn render_watch_screen(f: &mut Frame, state: &AppState, area: Rect) {
    let theme = theme();
    let snapshot = state.watch_snapshot.as_ref();

    let control_label = if state.watch_control_enabled { " [CONTROL]" } else { " [READ-ONLY]" };
    let age_label = state
        .watch_last_refresh
        .map(|ts| {
            let secs = ts.elapsed().as_secs();
            if secs < 5 { "now".to_string() } else { format!("{}s ago", secs) }
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
        let banner = Paragraph::new(format!("  ⚠  COORDINATOR PAUSED — {} ", pause_reason))
            .style(Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD));
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
        (true, true) => vec![Constraint::Percentage(40), Constraint::Percentage(30), Constraint::Min(3)],
        (true, false) => vec![Constraint::Percentage(80), Constraint::Length(0), Constraint::Min(3)],
        (false, true) => vec![Constraint::Length(0), Constraint::Percentage(90), Constraint::Min(3)],
        (false, false) => vec![Constraint::Percentage(40), Constraint::Percentage(30), Constraint::Min(3)],
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
                                let secs =
                                    (now_rfc - hb.with_timezone(&chrono::Utc)).num_seconds();
                                if secs < 60 { format!("{}s", secs) } else { format!("{}m", secs / 60) }
                            })
                            .unwrap_or_else(|| "-".to_string());

                        let text = format!(
                            "{} {}  {}  {}  {}  hb {}",
                            symbol, w.id, task, phase, w.runtime_status, hb_age
                        );
                        let style = if sel {
                            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
                        } else if freshly_stale || symbol == "▲" {
                            Style::default().fg(Color::Yellow)
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
        let workers_list = List::new(worker_lines)
            .block(Block::default().title("Workers  ↑↓ navigate").borders(Borders::ALL));
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
            if events_text.is_empty() { "No events" } else { &events_text },
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
            if log_text.is_empty() { "No log tail — 'f' to follow coordinator logs" } else { &log_text },
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
    let strip_text = format!(
        "{}{} | ↑↓ workers | r refresh | '?' help | q quit",
        title, throttled_label
    );
    let strip =
        Paragraph::new(strip_text).block(Block::default().title("Status").borders(Borders::ALL));
    f.render_widget(strip, vertical[2]);
}

fn render_help_overlay(f: &mut Frame, state: &AppState) {
    let area = ui::centered_rect(60, 60, f.size());
    f.render_widget(Clear, area); // Clear the background

    let current_screen = state.current_screen();
    let help_items = current_screen.help_keybindings();

    let mut text = format!("Help: {}\n\n", current_screen.title());
    for (key, desc) in help_items {
        text.push_str(&format!("{:<15} : {}\n", key, desc));
    }

    let help_para = Paragraph::new(text)
        .block(
            Block::default()
                .title("Keybindings")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
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
        None => ("<no coordinator process>".to_string(), 0usize),
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

fn footer_hints_line(state: &AppState, theme: &ui::Theme, max_chars: usize) -> Line<'static> {
    if state.current_screen() != Screen::CoordinatorLive {
        return Line::from(vec![
            Span::styled("Hints: ", Style::default().fg(theme.muted)),
            Span::raw(compact_help_line(
                state.current_screen().help_keybindings(),
                max_chars,
            )),
            Span::raw("  "),
            Span::styled("Press ?", Style::default().fg(theme.accent)),
            Span::raw(" for help"),
        ]);
    }

    let is_viewer = !state.coordinator_ownership.is_owner;
    let has_pending = state
        .coordinator_ownership
        .pending_incoming_request
        .is_some();
    let bindings = vec![
        ("r", "Run Full Cycle", is_viewer),
        ("y", "Sync Registry", is_viewer),
        ("c", "Reconcile", is_viewer),
        ("u", "Resume Paused Run", is_viewer),
        ("k", "Stop Options", is_viewer),
        ("v", "Recover Options", is_viewer),
        ("l", "Refresh Live Status", false),
        ("T", "Request Takeover", !is_viewer),
        ("a/r", "Accept / Reject", !has_pending),
    ];

    let mut spans = Vec::new();
    let mut used = 0usize;
    for (idx, (key, desc, disabled)) in bindings.into_iter().enumerate() {
        let chunk = if idx == 0 {
            format!("{key}: {desc}")
        } else {
            format!(" | {key}: {desc}")
        };
        let chunk_len = chunk.chars().count();
        if used + chunk_len > max_chars {
            break;
        }
        if idx > 0 {
            spans.push(Span::raw(" | "));
        }
        let style = if disabled {
            Style::default().fg(theme.muted)
        } else {
            Style::default()
        };
        spans.push(Span::styled(key.to_string(), style));
        spans.push(Span::styled(": ", style));
        spans.push(Span::styled(desc.to_string(), style));
        used += chunk_len;
    }

    spans.push(Span::raw("  "));
    spans.push(Span::styled("Press ?", Style::default().fg(theme.accent)));
    spans.push(Span::raw(" for help"));

    let mut with_label = vec![Span::styled("Hints: ", Style::default().fg(theme.muted))];
    with_label.extend(spans);
    Line::from(with_label)
}

#[cfg(test)]
mod tests {
    use super::handle_key;
    use crate::screen::Screen;
    use crate::state::{AppState, UiStatusLevel};
    use crossterm::event::KeyCode;
    use macc_core::catalog::{Agent, Skill};
    use macc_core::config::CanonicalConfig;
    use macc_core::doctor::ToolCheck;
    use macc_core::plan::{ActionPlan, PlannedOp};
    use macc_core::process_ownership::{ClientIdentity, ClientKind, ProcessHandle, ProcessKind};
    use macc_core::resolve::MaterializedFetchUnit;
    use macc_core::service::process_ownership::{claim_owner, register_process};
    use macc_core::tool::{ToolDescriptor, ToolDiagnostic};
    use macc_core::{Engine, ProjectPaths};
    use std::cell::RefCell;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[derive(Default)]
    struct ViewerGateEngine {
        stop_calls: RefCell<usize>,
    }

    impl Engine for ViewerGateEngine {
        fn list_tools(&self, _paths: &ProjectPaths) -> (Vec<ToolDescriptor>, Vec<ToolDiagnostic>) {
            (Vec::new(), Vec::new())
        }

        fn doctor(&self, _paths: &ProjectPaths) -> Vec<ToolCheck> {
            Vec::new()
        }

        fn plan(
            &self,
            _paths: &ProjectPaths,
            _config: &CanonicalConfig,
            _materialized_units: &[MaterializedFetchUnit],
            _overrides: &macc_core::resolve::CliOverrides,
        ) -> macc_core::Result<ActionPlan> {
            Ok(ActionPlan::default())
        }

        fn plan_operations(&self, _paths: &ProjectPaths, _plan: &ActionPlan) -> Vec<PlannedOp> {
            Vec::new()
        }

        fn apply(
            &self,
            _paths: &ProjectPaths,
            _plan: &mut ActionPlan,
            _allow_user_scope: bool,
        ) -> macc_core::Result<macc_core::ApplyReport> {
            Ok(macc_core::ApplyReport::default())
        }

        fn builtin_skills(&self) -> Vec<Skill> {
            Vec::new()
        }

        fn builtin_agents(&self) -> Vec<Agent> {
            Vec::new()
        }

        fn coordinator_stop_managed_command_process(
            &self,
            _paths: &ProjectPaths,
            _graceful: bool,
        ) -> macc_core::Result<macc_core::service::coordinator::CoordinatorStopResult> {
            *self.stop_calls.borrow_mut() += 1;
            Ok(macc_core::service::coordinator::CoordinatorStopResult {
                targets: 0,
                used_group: false,
            })
        }
    }

    fn sample_cli_client(client_id: &str) -> ClientIdentity {
        let now = chrono::Utc::now().to_rfc3339();
        ClientIdentity {
            client_id: client_id.to_string(),
            kind: ClientKind::Cli,
            connected_at: now.clone(),
            last_heartbeat: now,
        }
    }

    fn sample_project(dir: &std::path::Path) {
        let macc_dir = dir.join(".macc");
        fs::create_dir_all(&macc_dir).expect("create .macc");
        fs::write(macc_dir.join("macc.yaml"), "tools:\n  enabled: []\n").expect("write config");
    }

    #[test]
    fn coordinator_live_viewer_key_shows_viewer_mode_toast() {
        let dir = tempdir().expect("tempdir");
        sample_project(dir.path());
        let handle = ProcessHandle {
            kind: ProcessKind::Coordinator,
            project_root: dir.path().to_path_buf(),
            pid: Some(4242),
        };
        register_process(dir.path(), handle.clone()).expect("register process");
        claim_owner(dir.path(), &handle, sample_cli_client("client-A")).expect("claim owner");

        let engine = Arc::new(ViewerGateEngine::default());
        let mut state = AppState::with_engine(engine.clone());
        state.project_paths = Some(ProjectPaths::from_root(dir.path()));
        state.client_identity.client_id = "client-B".to_string();
        state.client_context.client_id = "client-B".to_string();
        state.client_context.project_root = dir.path().to_path_buf();
        state.goto_screen(Screen::CoordinatorLive);

        handle_key(&mut state, KeyCode::Char('K'));

        let status = state.ui_status.expect("status");
        assert_eq!(status.level, UiStatusLevel::Warning);
        assert_eq!(status.message, "Viewer mode — press T to request takeover");
        assert_eq!(*engine.stop_calls.borrow(), 0);
    }
}

fn render_coordinator_pause_overlay(f: &mut Frame, state: &AppState) {
    let area = ui::centered_rect(75, 45, f.size());
    f.render_widget(Clear, area);
    let message = state
        .coordinator_pause_error
        .as_deref()
        .unwrap_or("Coordinator paused due to an error.");
    let command_name = state.coordinator_pause_command.as_deref().unwrap_or("run");
    let retry_target = match (
        state.coordinator_pause_task_id.as_deref(),
        state.coordinator_pause_phase.as_deref(),
    ) {
        (Some(task), Some(phase)) => format!("task={} phase={}", task, phase),
        (Some(task), None) => format!("task={} phase=dev", task),
        _ => "global/blocking (no task context)".to_string(),
    };
    // RL-TUI-007: show a specific banner when quota is exhausted (E602).
    let is_quota_error = message.contains("quota_exhausted")
        || message.contains("E602")
        || message.to_ascii_lowercase().contains("quota exhausted");
    let (title, text) = if is_quota_error {
        (
            "Coordinator Paused — Quota Exhausted",
            format!(
                "PAUSED: Quota exhausted.\n\n{}\n\nTarget:\n- {}\n\nOptions:\n- Wait for quota reset, then press 'u' to resume\n- Switch to another tool in your macc config, then press 'u'\n- Press 'r' or Enter: retry failed phase\n- Press 's': skip failed phase (move task to todo)\n- Press 'k' or Esc: stop and keep paused state\n\nCommand: {}\n",
                message, retry_target, command_name
            ),
        )
    } else {
        (
            "Coordinator Error",
            format!(
                "Coordinator Paused (blocking error)\n\n{}\n\nTarget:\n- {}\n\nFix the issue in your repo/worktree, then choose:\n\n- Press 'r' or Enter: retry failed phase, then resume run\n- Press 's': skip failed phase (move task to todo), then resume run\n- Press 'u': send manual resume signal (same as `macc coordinator resume`)\n- Press 'o': open Logs screen\n- Press 'k' or Esc: stop and keep paused state\n- Press 'c': resume run without retry\n\nCommand: {}\n",
                message, retry_target, command_name
            ),
        )
    };
    let popup = Paragraph::new(text)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red)),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(popup, area);
}

fn render_coordinator_stop_dialog(f: &mut Frame, state: &AppState) {
    let area = ui::centered_rect(65, 40, f.size());
    f.render_widget(Clear, area);
    let theme = ui::theme();

    let options = [
        "Drain & Stop: Finish active tasks, then stop.",
        "Graceful Stop: Stop active performers at next safe boundary.",
        "Force Stop: Terminate processes immediately.",
        "Force Stop + Cleanup: Terminate and delete worktrees/branches.",
    ];

    let mut text = Vec::new();
    text.push(Line::from("Select a stop mode:"));
    text.push(Line::from(""));

    for (idx, opt) in options.iter().enumerate() {
        let style = if idx == state.coordinator_stop_dialog_selection {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
                .bg(theme.highlight_bg)
        } else {
            Style::default()
        };
        let prefix = if idx == state.coordinator_stop_dialog_selection {
            "> "
        } else {
            "  "
        };
        text.push(Line::from(vec![Span::styled(format!("{}{}", prefix, opt), style)]));
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

fn render_coordinator_recover_dialog(f: &mut Frame, state: &AppState) {
    let area = ui::centered_rect(65, 30, f.size());
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
                .bg(theme.highlight_bg)
        } else {
            Style::default()
        };
        let prefix = if idx == state.coordinator_recover_dialog_selection {
            "> "
        } else {
            "  "
        };
        text.push(Line::from(vec![Span::styled(format!("{}{}", prefix, opt), style)]));
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
        let lines: Vec<String> = reader.lines().filter_map(Result::ok).collect();
        let start = lines.len().saturating_sub(limit);
        lines[start..].to_vec()
    } else {
        Vec::new()
    }
}

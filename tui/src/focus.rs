use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Layer {
    Launch,
    Apply,
    TaskDetail,
    Decision,
    Approval,
    Finished,
    Paused,
    Queue,
    Search,
    ToolField,
    Priority,
    Parallel,
    Supervisor,
    ConfigField,
    Install,
    Stop,
    StopTask,
    Recovery,
    Takeover,
    Hooks,
    Workspace,
    Page,
}
impl Layer {
    pub(super) fn accepts_input(self, s: &AppState) -> bool {
        match self {
            Self::Launch
            | Self::Apply
            | Self::Decision
            | Self::Approval
            | Self::Queue
            | Self::Search
            | Self::ConfigField
            | Self::ToolField
            | Self::Supervisor => true,
            Self::Workspace => s.sessions.input_active(),
            Self::Hooks => s.hooks.input_active(),
            _ => false,
        }
    }
    pub(super) fn current(s: &AppState) -> Self {
        use Layer::*;
        let page = s.current_screen();
        if s.launch_dialog.is_some() {
            Launch
        } else if page == Screen::Apply && s.apply_confirmation_open() {
            Apply
        } else if s.decision_center.open {
            Decision
        } else if s.approval_window.is_some() {
            Approval
        } else if matches!(page, Screen::Sessions | Screen::O2) && s.sessions.input_active() {
            Workspace
        } else if page == Screen::CoordinatorLive && s.coordinator_recover_dialog_open {
            Recovery
        } else if page == Screen::CoordinatorLive && s.coordinator_stop_task_confirm_id.is_some() {
            StopTask
        } else if page == Screen::CoordinatorLive && s.coordinator_stop_dialog_open {
            Stop
        } else if s.coordinator_ownership.pending_incoming_request.is_some() {
            Takeover
        } else if s.has_coordinator_finished_prompt() {
            Finished
        } else if s.has_coordinator_pause_prompt() {
            Paused
        } else if s.prd_queue_editor.is_some() {
            Queue
        } else if page == Screen::CoordinatorLive
            && (s.coordinator_task_diff_popup.is_some()
                || s.coordinator_task_explain_popup.is_some())
        {
            TaskDetail
        } else if s.search_editing {
            Search
        } else if page == Screen::ToolSettings && s.is_tool_field_editing() {
            ToolField
        } else if page == Screen::Tools && s.is_tool_install_confirmation_open() {
            Install
        } else if page == Screen::Automation && s.tool_priority_editor_active {
            Priority
        } else if page == Screen::Automation && s.tool_parallel_editor_active {
            Parallel
        } else if page == Screen::Automation && s.supervisor_commands_active() {
            Supervisor
        } else if page == Screen::Automation
            && (s.automation_field_editing || s.settings_field_editing)
        {
            ConfigField
        } else if page == Screen::Hooks {
            Hooks
        } else if matches!(page, Screen::O2 | Screen::Sessions) {
            Workspace
        } else {
            Page
        }
    }
}
pub(super) fn widget_input(state: &mut AppState, key: KeyCode) {
    match Layer::current(state) {
        Layer::Launch => {
            state.launch_key(key);
        }
        Layer::Apply => {
            state.apply_key(key);
        }
        Layer::TaskDetail => match key {
            KeyCode::PageDown => state.inspector_scroll = state.inspector_scroll.saturating_add(5),
            KeyCode::PageUp => state.inspector_scroll = state.inspector_scroll.saturating_sub(5),
            KeyCode::Esc => {
                state.coordinator_task_diff_popup = None;
                state.coordinator_task_explain_popup = None;
            }
            _ => {}
        },
        Layer::Decision => {
            if let Some(action) = state.decision_center.key_event(key) {
                if let Some(paths) = &state.project_paths {
                    state.decision_center.submit(
                        paths.root.clone(),
                        state.client_identity.client_id.clone(),
                        action,
                    );
                }
            }
        }
        Layer::Approval => {
            if let Some(window) = state.approval_window.as_mut() {
                match window.key(key) {
                    approval::Outcome::Continue => {}
                    approval::Outcome::Later => state.snooze_approval_window(),
                    approval::Outcome::Next => state.next_approval_window(),
                    approval::Outcome::Submit(decision) => state.submit_approval(decision),
                }
            }
        }
        Layer::Finished => match key {
            KeyCode::Enter | KeyCode::Esc => state.dismiss_coordinator_finished(),
            _ => {}
        },
        Layer::Paused
            if state
                .coordinator_pause_summary
                .as_ref()
                .is_some_and(|s| s.scoped_prd.is_some()) =>
        {
            match key {
                KeyCode::Char('o') => state.open_logs_after_coordinator_pause(),
                KeyCode::Esc => state.stop_after_coordinator_pause(),
                _ => {}
            }
        }
        Layer::Paused => match key {
            KeyCode::Char('r') | KeyCode::Enter => state.retry_after_coordinator_pause(),
            KeyCode::Char('s') => state.skip_after_coordinator_pause(),
            KeyCode::Char('o') => state.open_logs_after_coordinator_pause(),
            KeyCode::Char('d') => {
                let task = state.coordinator_pause_task_id.clone().or_else(|| {
                    state
                        .coordinator_run_summary
                        .as_ref()
                        .and_then(|s| s.task_id.clone())
                });
                state.decision_center.show_for_task(task.as_deref());
            }
            KeyCode::Char('u') => state.resume_signal_after_coordinator_pause(),
            KeyCode::Esc => state.stop_after_coordinator_pause(),
            KeyCode::Char('c') => state.resume_after_coordinator_pause(),
            _ => {}
        },
        Layer::Queue => {
            if let Some(editor) = &mut state.prd_queue_editor {
                match editor.key(key) {
                    prd_queue::Outcome::Continue => {}
                    prd_queue::Outcome::Cancel => state.prd_queue_editor = None,
                    prd_queue::Outcome::Reset => {
                        let result = state.reset_prd_queue_progress();
                        if let Some(editor) = &mut state.prd_queue_editor {
                            editor.report_reset(result);
                        }
                    }
                    prd_queue::Outcome::Save => {
                        let files = editor.files.clone();
                        state.snapshot_before_config_change();
                        if let Some(config) = &mut state.working_copy {
                            config
                                .automation
                                .coordinator
                                .get_or_insert_with(Default::default)
                                .prd_files = files;
                        }
                        state.prd_queue_editor = None;
                    }
                }
            }
        }
        Layer::Search => match key {
            KeyCode::Enter => state.commit_search(),
            KeyCode::Esc => state.cancel_search(),
            KeyCode::Backspace => state.pop_search_char(),
            KeyCode::Char(c) => state.append_search_char(c),
            _ => {}
        },
        Layer::ToolField => match key {
            KeyCode::Enter => state.commit_tool_field_edit(),
            KeyCode::Esc => state.cancel_tool_field_edit(),
            KeyCode::Backspace => state.pop_tool_field_char(),
            KeyCode::Char(c) => state.append_tool_field_char(c),
            _ => {}
        },
        Layer::Priority => {
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
        }
        Layer::Parallel => {
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
                // Esc — cancel (changes already applied in-place; u restores the previous snapshot)
                KeyCode::Esc => state.cancel_tool_parallel_editor(),
                _ => {}
            }
        }
        Layer::Supervisor => {
            state.supervisor_commands_key(key);
        }
        Layer::ConfigField => match key {
            KeyCode::Enter => state.commit_config_edit(),
            KeyCode::Esc => state.cancel_config_edit(),
            KeyCode::Backspace => state.pop_config_char(),
            KeyCode::Char(c) => state.append_config_char(c),
            _ => {}
        },
        Layer::Install => match key {
            KeyCode::Enter => state.confirm_tool_install(),
            KeyCode::Esc => state.cancel_tool_install_confirmation(),
            _ => {}
        },
        Layer::Stop => match key {
            KeyCode::Up => {
                state.coordinator_stop_dialog_selection =
                    state.coordinator_stop_dialog_selection.saturating_sub(1);
            }
            KeyCode::Down if state.coordinator_stop_dialog_selection < 3 => {
                state.coordinator_stop_dialog_selection += 1;
            }
            KeyCode::Enter => {
                state.stop_coordinator_with_selected_mode();
            }
            KeyCode::Esc => {
                state.close_coordinator_stop_dialog();
            }
            _ => {}
        },
        Layer::StopTask => match key {
            KeyCode::Enter => {
                if let Some(task_id) = state.coordinator_stop_task_confirm_id.take() {
                    state.stop_selected_task(task_id.clone());
                    state.set_status(
                        UiStatusLevel::Info,
                        format!("Sent stop request to task {}", task_id),
                        Some(Duration::from_secs(3)),
                    );
                }
            }
            KeyCode::Esc => {
                state.coordinator_stop_task_confirm_id = None;
                state.set_status(
                    UiStatusLevel::Info,
                    "Stop task cancelled.",
                    Some(Duration::from_secs(2)),
                );
            }
            _ => {}
        },
        Layer::Recovery => match key {
            KeyCode::Up => {
                state.coordinator_recover_dialog_selection =
                    state.coordinator_recover_dialog_selection.saturating_sub(1);
            }
            KeyCode::Down if state.coordinator_recover_dialog_selection < 1 => {
                state.coordinator_recover_dialog_selection += 1;
            }
            KeyCode::Enter => {
                state.recover_coordinator_with_selected_mode();
            }
            KeyCode::Esc => {
                state.close_coordinator_recover_dialog();
            }
            _ => {}
        },
        Layer::Takeover => match key {
            KeyCode::Char('a') => state.ownership_respond_takeover(true),
            KeyCode::Char('r') => state.ownership_respond_takeover(false),
            KeyCode::Esc => state.dismiss_takeover_request_modal(),
            _ => {}
        },
        Layer::Workspace => {
            session_navigation::handle(state, key);
        }
        Layer::Hooks => {
            hooks::handle(state, key);
        }
        Layer::Page if state.current_screen() == Screen::Apply => state.apply_key(key),
        Layer::Page => {}
    }
}

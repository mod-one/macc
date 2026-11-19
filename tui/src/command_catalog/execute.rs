use super::*;
use crate::navigation::Destination;
use pages::Action::*;
pub(crate) fn execute(s: &mut AppState, action: Action) {
    match action {
        Overlay(a) => overlays::execute(s, a),
        Help => s.toggle_help(),
        Palette => open_command_palette(s),
        SwitchMode => crate::navigation::switch(s),
        Go(dest) => dest.open(s),
        Quit => s.should_quit = true,
        Back => {
            if s.screen_stack.len() > 1 {
                s.pop_screen()
            } else {
                s.should_quit = true
            }
        }
        Search => s.begin_search(),
        Widget(key) => crate::widget_input(s, key),
        Run | Resume | Sync | Reconcile | StopRun | Recover => {
            let effect = |s: &mut AppState| match action {
                Run => s.start_coordinator_command(CoordinatorCommand::Run),
                Resume => s.start_coordinator_command(CoordinatorCommand::ResumePausedRun),
                Sync => s.start_coordinator_command(CoordinatorCommand::SyncRegistry),
                Reconcile => s.start_coordinator_command(CoordinatorCommand::ReconcileRuntime),
                Recover => s.open_coordinator_recover_dialog(),
                _ => s.open_coordinator_stop_dialog(),
            };
            if let Some(h) = s.coordinator_handle() {
                s.try_owner_action(&h, effect);
            } else {
                effect(s)
            }
        }
        StopTask => {
            s.coordinator_stop_task_confirm_id = s.selected_live_task().map(|t| t.task_id);
        }
        RetryTask => {
            if let Some(t) = s.selected_live_task() {
                let result = s.requeue_selected_task(t.task_id.clone());
                let (level, msg) = match result {
                    Ok(_) => (UiStatusLevel::Success, format!("Requeued {}", t.task_id)),
                    Err(e) => (UiStatusLevel::Error, e.to_string()),
                };
                s.set_status(level, msg, Some(Duration::from_secs(4)));
            }
        }
        Takeover => s.ownership_request_takeover(),
        Explain | Diff => {
            s.inspector_scroll = 0;
            if let Some(t) = s.selected_live_task() {
                if action == Diff {
                    s.coordinator_task_diff_popup = Some(s.get_task_diff(&t))
                } else {
                    s.coordinator_task_explain_popup = Some(s.get_task_explain(&t))
                }
            }
        }
        LogPane => s.toggle_log_pane(),
        Doctor => s.run_home_doctor_check(),
        SupervisorInbox => {
            if let Some(index) = crate::inbox::supervisor_decision_index(s) {
                Destination::Inbox.open(s);
                s.inbox_section = crate::inbox::Section::Decisions;
                s.inspector_index = index;
            }
        }
        SupervisorReport => {
            s.inspector_scroll = 0;
            if let Some(live) = s
                .observations
                .latest
                .as_ref()
                .and_then(|o| o.intervention.as_ref())
            {
                s.coordinator_task_explain_popup = Some(
                    supervisor::detail(live, chrono::Utc::now())
                        .into_iter()
                        .map(|(k, v, _)| format!("{k}{v}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
        }
        ToolSection(installation) => {
            if s.current_screen() == Screen::ToolSettings {
                s.pop_screen();
            }
            s.tool_installation_tab = installation;
        }
        ToolPane => {
            if s.current_screen() == Screen::ToolSettings {
                s.pop_screen();
            } else {
                s.tool_installation_tab = false;
                s.navigate_enter();
            }
        }
        ConfigSection(idx) => s.jump_config_tab(idx),
        ConfigEdit => s.begin_current_config_edit(),
        PolicyApproval => s.decision_settings_challenge(),
        AdjustConfig(forward) => match s.current_config_field() {
            Some((1, 0)) => s.cycle_coordinator_tool(forward),
            Some((2, idx)) => s.adjust_supervisor_field(idx, forward),
            Some((3, idx)) => s.adjust_decision_field(idx, forward),
            _ => {}
        },
        EditChange => crate::workbench::edit_change(s),
        WorkspaceRaw => s.sessions.toggle_raw(),
        DoctorRaw => s.doctor_raw = !s.doctor_raw,
        OverviewSection(activity) => {
            s.home_activity = activity;
            s.inspector_index = s
                .observations
                .latest
                .as_ref()
                .filter(|_| !activity)
                .and_then(|o| crate::overview_view::next_step(&o.readiness))
                .unwrap_or(0);
            s.inspector_scroll = 0;
        }
        OpenReadiness => {
            if let Some(step) = s
                .observations
                .latest
                .as_ref()
                .and_then(|o| o.readiness.steps.get(s.inspector_index))
            {
                let dest = match step.number {
                    2 => Destination::Tools,
                    3 => Destination::Changes,
                    4 => Destination::Coordinator,
                    6.. => Destination::Live,
                    _ => Destination::Doctor,
                };
                dest.open(s);
            }
        }
        OpenInboxRow => crate::inbox::open(s),
        InboxSection(section) => {
            s.inbox_section = section;
            s.inspector_index = 0;
            s.inspector_scroll = 0;
        }
        InspectRaw => {
            s.inbox_raw = !s.inbox_raw;
            s.inspector_scroll = 0;
        }
        InspectorScroll(down) => {
            s.inspector_scroll = if down {
                s.inspector_scroll.saturating_add(5)
            } else {
                s.inspector_scroll.saturating_sub(5)
            };
        }

        Save => s.save_config(),
        Undo => s.undo_config_change(),
        Redo => s.redo_config_change(),
        Preview if s.current_screen() == Screen::Preview => {}
        Preview => s.open_preview(),
        Apply if s.current_screen() == Screen::Apply => {}
        Apply => s.open_apply_screen(),
        Previous | Next
            if matches!(
                s.current_screen(),
                Screen::Home | Screen::Doctor | Screen::Changes | Screen::Inbox
            ) =>
        {
            s.inspector_scroll = 0;
            if action == Next {
                s.inspector_index = s
                    .inspector_index
                    .saturating_add(1)
                    .min(crate::workbench::row_count(s).saturating_sub(1));
            } else {
                s.inspector_index = s.inspector_index.saturating_sub(1);
            }
        }
        Previous => s.navigate_prev(),
        Next => s.navigate_next(),
        Toggle => s.navigate_toggle(),
        EnterPage => s.navigate_enter(),
        PageScroll(down) => {
            let by = if down { 10 } else { -10 };
            if s.current_screen() == Screen::Preview {
                s.scroll_preview_diff(by)
            } else {
                s.scroll_log_content(by)
            }
        }
        SelectAll(all) => match (s.current_screen(), all) {
            (Screen::Skills, true) => s.select_all_skills(),
            (Screen::Skills, false) => s.select_no_skills(),
            (Screen::Agents, true) => s.select_all_agents(),
            (Screen::Agents, false) => s.select_no_agents(),
            (_, true) => s.select_all_mcp(),
            (_, false) => s.select_no_mcp(),
        },
        Refresh => match s.current_screen() {
            Screen::Tools => s.refresh_tool_checks(),
            Screen::Watch => s.refresh_watch_snapshot(),
            Screen::Logs => s.refresh_logs(),
            Screen::Preview => s.refresh_preview_plan(),
            Screen::Skills => {
                if let Some(p) = &s.project_paths {
                    macc_core::catalog::availability::schedule_refresh(p, true);
                }
                s.refresh_skills();
            }
            _ => {}
        },
        Install => s.begin_tool_install_confirmation(),
        Context => s.generate_context_for_selected_tool(),
        Cooldown => {
            if let Some(q) = crate::doctor_view::cooldown(s) {
                s.reset_tool_cooldown(q.tool_id.clone());
            }
        }
    }
}

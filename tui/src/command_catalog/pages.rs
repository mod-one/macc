//! Page actions and their bindings. Focused editors own their text and cursor input.
use super::*;
pub(crate) use crate::navigation::Destination;
use KeyCode::{Char as C, *};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Widget(KeyCode),
    Go(Destination),
    Help,
    Overlay(super::overlays::OverlayAction),
    Palette,
    SwitchMode,
    Back,
    Quit,
    Search,
    Run,
    Resume,
    Sync,
    Reconcile,
    Recover,
    StopRun,
    StopTask,
    RetryTask,
    Takeover,
    Explain,
    Diff,
    LogPane,
    Doctor,
    OpenInboxRow,
    OverviewSection(bool),
    OpenReadiness,
    InboxSection(crate::inbox::Section),
    InspectRaw,
    WorkspaceRaw,
    InspectorScroll(bool),
    Save,
    Undo,
    Redo,
    Preview,
    Apply,
    Previous,
    Next,
    Toggle,
    EnterPage,
    PageScroll(bool),
    SelectAll(bool),
    Refresh,
    Install,
    Context,
    Cooldown,
    DoctorRaw,
    EditChange,
    ConfigSection(usize),
    AdjustConfig(bool),
    ConfigEdit,
    PolicyApproval,
    ToolPane,
    ToolSection(bool),
    SupervisorReport,
    SupervisorInbox,
}
pub(super) fn commands(s: &AppState) -> Vec<Command> {
    use Action as A;
    let screen = s.current_screen();
    let mut cs = Vec::new();
    let mut add = |k, label, a| cs.push(Command::bound(k, label, a));
    let mut evidence = None;
    let mut selection = false;
    match screen {
        Screen::Home => {
            add(C('r'), "Run coordinator", A::Run);
            add(C('d'), "Run doctor", A::Go(Destination::Doctor));
            add(C('1'), "Readiness", A::OverviewSection(false));
            add(C('2'), "Activity", A::OverviewSection(true));
            add(Tab, "Next section", A::OverviewSection(!s.home_activity));
            add(
                BackTab,
                "Previous section",
                A::OverviewSection(!s.home_activity),
            );
            if !s.home_activity {
                add(Enter, "Open selected step", A::OpenReadiness);
                selection = true;
            }
        }
        Screen::CoordinatorLive => {
            add(C('r'), "Run coordinator", A::Run);
            add(C('u'), "Resume retained work", A::Resume);
            add(C('t'), "Retry selected task", A::RetryTask);
            add(C('x'), "Stop selected task…", A::StopTask);
            if s.live_supervisor().is_some() && s.selected_live_task().is_none() {
                if crate::inbox::supervisor_decision_index(s).is_some() {
                    add(Enter, "Answer in Inbox", A::SupervisorInbox);
                } else {
                    add(
                        Enter,
                        "Inspect supervisor intervention",
                        A::SupervisorReport,
                    );
                }
            } else {
                add(Enter, "Inspect selected task", A::Explain);
            }

            add(C('d'), "Task diff", A::Diff);
            add(C('v'), "Recovery options", A::Recover);
            add(C('c'), "Reconcile", A::Reconcile);
            add(C('y'), "Sync registry", A::Sync);
            add(C('k'), "Stop coordinator…", A::StopRun);
            add(C('o'), "Request ownership", A::Takeover);
            add(C('l'), "Toggle logs", A::LogPane);
            add(C('i'), "Supervisor report", A::SupervisorReport);
            evidence = Some(Command::ctrl(
                'r',
                "Inspect recorded evidence",
                A::SupervisorReport,
            ));
            add(C('1'), "Live tasks", A::Go(Destination::Live));
            add(C('2'), "Workers", A::Go(Destination::Workers));
            add(C('/'), "Find task", A::Search);
            selection = true;
        }
        Screen::Inbox => {
            use crate::inbox::Section::*;
            add(Enter, "Open selected request", A::OpenInboxRow);
            for (key, section, label) in [
                (C('1'), Decisions, "Decisions"),
                (C('2'), Approvals, "Approvals"),
                (C('3'), History, "History"),
                (Tab, s.inbox_section.next(false), "Next section"),
                (BackTab, s.inbox_section.next(true), "Previous section"),
            ] {
                add(key, label, A::InboxSection(section));
            }
            evidence = Some(Command::ctrl('r', "Toggle evidence", A::InspectRaw));
            selection = true;
        }
        Screen::Doctor => {
            add(C('r'), "Run checks", A::Doctor);
            if crate::doctor_view::cooldown(s).is_some() {
                add(Enter, "Clear selected tool cooldown", A::Cooldown);
            }
            evidence = Some(Command::ctrl('r', "Toggle evidence", A::DoctorRaw));
            selection = true;
        }
        Screen::Changes => {
            add(Enter, "Open settings section", A::EditChange);
            add(C('p'), "Preview file changes", A::Preview);
            add(C('a'), "Review Apply…", A::Apply);
            selection = true;
        }
        Screen::Automation => {
            if s.config_tab_index != 0 {
                let tabs = crate::automation_view::SECTIONS;
                for (i, &(id, label)) in tabs.iter().enumerate() {
                    add(C(char::from(b'1' + i as u8)), label, A::ConfigSection(id));
                }
                let pos = tabs
                    .iter()
                    .position(|&(id, _)| id == s.config_tab_index)
                    .unwrap_or(0);
                add(
                    Tab,
                    "Next section",
                    A::ConfigSection(tabs[(pos + 1) % tabs.len()].0),
                );
                add(
                    BackTab,
                    "Previous section",
                    A::ConfigSection(tabs[(pos + tabs.len() - 1) % tabs.len()].0),
                );
            }
            if matches!(s.current_config_field(), Some((1, 0) | (2 | 3, _))) {
                add(Left, "Previous value", A::AdjustConfig(false));
                add(Right, "Next value", A::AdjustConfig(true));
            }
            if matches!(s.current_config_field(), Some((2 | 3, _))) {
                add(C('e'), "Edit exact value", A::ConfigEdit);
            }
            if s.config_tab_index == 7 {
                add(C('a'), "Prepare policy approval", A::PolicyApproval);
            }
            add(Enter, "Edit setting", A::EnterPage);
            add(C(' '), "Toggle or edit", A::Toggle);
            selection = true;
        }
        Screen::Tools => {
            add(Enter, "Edit tool settings", A::EnterPage);
            add(C(' '), "Toggle tool", A::Toggle);
            add(C('i'), "Install tool…", A::Install);
            add(C('d'), "Refresh availability", A::Refresh);
            add(C('f'), "Generate context", A::Context);
            selection = true;
        }
        Screen::ToolSettings => {
            add(Enter, "Edit field", A::EnterPage);
            add(C(' '), "Toggle or edit", A::Toggle);
            selection = true;
        }
        Screen::Skills | Screen::Agents | Screen::Mcp => {
            add(Enter, "Toggle selection", A::EnterPage);
            add(C(' '), "Toggle selection", A::Toggle);
            add(C('a'), "Select all", A::SelectAll(true));
            add(C('n'), "Select none", A::SelectAll(false));
            add(C('/'), "Find", A::Search);
            if screen == Screen::Skills {
                add(C('r'), "Recheck sources", A::Refresh);
                add(C('2'), "Agents", A::Go(Destination::Agents));
            }
            if screen == Screen::Agents {
                add(C('1'), "Skills", A::Go(Destination::Skills));
            }
            selection = true;
        }
        Screen::Preview | Screen::Logs => {
            for (k, down) in [(PageUp, false), (PageDown, true)] {
                add(
                    k,
                    if down { "Scroll down" } else { "Scroll up" },
                    A::PageScroll(down),
                );
            }
            add(C('r'), "Refresh", A::Refresh);
            if screen == Screen::Preview {
                add(C('a'), "Review Apply…", A::Apply);
            } else {
                add(C('/'), "Find log", A::Search);
            }
            selection = true;
        }
        Screen::Apply => {
            for (k, label) in [
                (Enter, "Review and confirm Apply…"),
                (Up, "Previous operation"),
                (Down, "Next operation"),
                (PageUp, "Scroll result up"),
                (PageDown, "Scroll result down"),
                (C('v'), "Toggle technical output"),
                (C('r'), "Refresh plan"),
                (Esc, "Back"),
            ] {
                add(k, label, A::Widget(k));
            }
        }
        Screen::Watch => {
            add(C('1'), "Live tasks", A::Go(Destination::Live));
            add(C('r'), "Refresh workers", A::Refresh);
            selection = true;
        }
        Screen::About | Screen::Hooks | Screen::Sessions | Screen::O2 => {}
    }
    if matches!(screen, Screen::Changes | Screen::Preview | Screen::Apply) {
        add(C('1'), "Draft", A::Go(Destination::Changes));
        add(C('2'), "Preview", A::Preview);
        add(C('3'), "Apply", A::Apply);
    }
    if matches!(screen, Screen::Tools | Screen::ToolSettings) {
        add(Tab, "Switch tool pane", A::ToolPane);
        add(BackTab, "Previous tool pane", A::ToolPane);
        add(C('1'), "Tool settings", A::ToolSection(false));
        add(C('2'), "Installation details", A::ToolSection(true));
        if screen == Screen::Tools {
            add(C('/'), "Find tool", A::Search);
        }
    }
    if matches!(screen, Screen::Home | Screen::Inbox | Screen::Doctor) {
        add(PageUp, "Scroll inspector up", A::InspectorScroll(false));
        add(PageDown, "Scroll inspector down", A::InspectorScroll(true));
    }
    if selection {
        add(Up, "Previous row", A::Previous);
        add(Down, "Next row", A::Next);
    }
    let live_task = (screen == Screen::CoordinatorLive)
        .then(|| s.selected_live_task())
        .flatten();
    let selected = live_task.is_some();
    let blocked = live_task
        .is_some_and(|t| t.workflow_state == macc_core::coordinator::WorkflowState::Blocked);
    let control = s.coordinator_ownership.is_owner
        || !s
            .coordinator_ownership
            .record
            .as_ref()
            .is_some_and(|r| r.owner.is_some());
    cs.extend(evidence);
    for c in &mut cs {
        c.enabled = match c.action {
            A::StopTask | A::RetryTask => selected && control && !blocked,
            A::Diff | A::Explain => selected,
            A::OpenInboxRow => {
                crate::inbox::count(s) > 0 || s.inbox_section == crate::inbox::Section::Decisions
            }
            A::Install => s
                .selected_tool_descriptor()
                .is_some_and(|t| t.install.is_some()),
            A::Cooldown => control && crate::doctor_view::cooldown(s).is_some(),
            A::Context if screen == Screen::Tools => s.selected_tool_descriptor().is_some(),
            A::SupervisorReport => s
                .observations
                .latest
                .as_ref()
                .is_some_and(|o| o.intervention.is_some()),
            A::ToolPane | A::EnterPage if screen == Screen::Tools => {
                s.selected_tool_descriptor().is_some_and(|t| {
                    s.working_copy
                        .as_ref()
                        .is_some_and(|c| c.tools.enabled.contains(&t.id))
                })
            }
            A::OpenReadiness => s
                .observations
                .latest
                .as_ref()
                .is_some_and(|o| !o.readiness.steps.is_empty()),
            A::EditChange => crate::workbench::row_count(s) > 0,
            A::Preview => s.preview_job.is_none(),
            A::Apply => {
                !s.apply_preparing() && !crate::workbench::has_draft(s) && s.preview_job.is_none()
            }
            A::Refresh if screen == Screen::Preview => s.preview_job.is_none(),
            A::Run => control && !s.is_coordinator_running(),
            A::Resume | A::Sync | A::Reconcile | A::Recover | A::StopRun => control,
            A::Takeover => !s.coordinator_ownership.is_owner,
            A::Widget(Enter) if screen == Screen::Apply => {
                s.apply_context.is_some() && !s.apply_preparing() && s.apply_feedback.is_none()
            }
            _ => true,
        };
    }
    for cmd in cs.iter_mut().filter(|c| !c.enabled && c.hint.is_empty()) {
        cmd.hint = match cmd.action {
            A::Cooldown => "Request ownership to clear a tool cooldown.",
            A::Run if s.is_coordinator_running() => "A coordinator is already running.",
            A::Run | A::Reconcile | A::Recover | A::Resume | A::StopRun | A::Sync => {
                "Request ownership to control this run."
            }
            A::StopTask | A::RetryTask if blocked => {
                "Inspect the outstanding prerequisite in the selected task detail."
            }
            A::StopTask | A::RetryTask if !control => "Request ownership to control a task.",
            A::StopTask | A::RetryTask | A::Diff | A::Explain => "Select a coordinator task.",
            A::Apply if s.preview_job.is_some() => "Wait for the preview to finish.",
            A::Apply => "Save or discard the outstanding drafts first.",
            A::EditChange => "No changed setting is selected.",
            A::SupervisorReport => "No supervisor intervention has been reported.",
            A::ToolPane | A::EnterPage if screen == Screen::Tools => "Select an enabled tool.",
            A::Install => "Select a tool with an installer.",
            _ => "This operation requires an eligible selection or a completed preparation.",
        }
        .into();
    }
    cs
}

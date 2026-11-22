use super::*;
use crate::workspace_tests::{export_buffer, fixture, render, text};
use macc_core::coordinator::{
    view_model::{LiveTaskRow, TaskHealth, TaskPhase},
    RuntimeStatus, WorkflowState,
};

fn state_with_worker_and_intervention() -> (tempfile::TempDir, AppState) {
    let (project, mut s) = fixture();
    navigation::Destination::Live.open(&mut s);
    s.coordinator_ownership.is_owner = true;
    s.coordinator_ownership.record = Some(serde_json::from_value(serde_json::json!({
        "process": {"kind":"Project","project_root":project.path(),"pid":null},
        "owner":{"kind":"Tui","client_id":"tui-preview","connected_at":""},"viewers":[],"started_at":""
    })).unwrap());
    s.observations.latest = Some(observations::Snapshot {
        readiness: macc_core::onboarding::ReadinessLadder { steps: vec![], blocking_count: 0 },
        trust: None, supervisor_status: Some("Supervisor waiting · intervention escalated".into()),
        intervention: Some(serde_json::from_value(serde_json::json!({
            "running":true,"active":false,"status":"escalated","attempt":2,"incident_id":"incident-42",
            "started_at": (chrono::Utc::now()-chrono::Duration::minutes(24)).to_rfc3339(),
            "phase_started_at": (chrono::Utc::now()-chrono::Duration::minutes(3)).to_rfc3339(),
            "task_ids":["BOOT-VERIFY"], "tool":"test-cli","reason":"Windows desktop verification requires an interactive runner.",
            "next_action":"Attach the Windows runner report.", "target_branch":"main","findings":[]
        })).unwrap()),
        prd: "bootstrap.json".into(),
    });
    s.coordinator_snapshot = Some(state::CoordinatorTaskSnapshot {
        total: 2,
        todo: 0,
        active: 1,
        blocked: 1,
        merged: 0,
        approved: 0,
        throttled_tools: vec![],
        task_rows: vec![LiveTaskRow {
            health: TaskHealth::Healthy,
            worker_id: "worker-01".into(),
            task_id: "BOOT-DOC".into(),
            workflow_state: WorkflowState::InProgress,
            runtime_status: RuntimeStatus::Running,
            phase: TaskPhase::Testing,
            tool: "test-cli".into(),
            model: "test-model".into(),
            effort: "high".into(),
            age: Duration::from_secs(80),
            heartbeat_age: Some(Duration::from_secs(3)),
            last_event_age: None,
            current_message: Some("Running documentation checks".into()),
            worktree: Some(".macc/worktree/worker-01".into()),
            branch: Some("docs/bootstrap".into()),
            last_error_code: None,
            last_error: None,
            result_explanation: None,
        }],
    });
    s.coordinator_run_summary = Some(serde_json::from_value(serde_json::json!({
        "run_id":"previous","status":"blocked","severity":"error","headline":"BOOT-VERIFY is waiting for desktop verification.",
        "cause":"The retained task requires a Windows runner.","task_id":"BOOT-VERIFY","occurred_at": chrono::Utc::now().to_rfc3339(),
        "next_action":"Review the verification request in Inbox.","dependent_task_ids":[],"repeated_count":1,"recent_runs":[]
    })).unwrap());
    (project, s)
}

#[test]
fn live_preserves_field_order_and_stacks_full_width_panes() {
    let (_project, mut s) = state_with_worker_and_intervention();
    for (width, height) in [(80, 24), (120, 36), (180, 48)] {
        let buffer = render(&s, width, height);
        let screen = text(&buffer);
        let lines: Vec<_> = screen.lines().collect();
        let position = |title: &str| {
            lines
                .iter()
                .enumerate()
                .find_map(|(i, l)| l.find(title).map(|x| (i, l[..x].chars().count())))
                .unwrap()
        };
        let (list, x) = position("LIVE TASKS");
        let (detail, dx) = position("SELECTED TASK DETAIL");
        let (result, rx) = position("LAST RESULT");
        assert!(list < detail && detail < result);
        assert_eq!((x, x), (dx, rx));
        let headings: Vec<_> = lines[list + 1].split_whitespace().collect();
        let header = headings.join(" ");
        assert!(
            header.contains("Health Worker Task ID Status Tool Model Effort Age HB"),
            "{screen}"
        );
        assert!(buffer.content.iter().any(|c| c.symbol() == "●"
            && c.fg == theme().good
            && c.bg == theme().operate_selection));
        export_buffer("live-worker", &s, width, height);
    }
    handle_key(&mut s, KeyCode::Char('l'));
    let screen = text(&render(&s, 120, 36));
    assert!(!screen.contains("LAST RESULT"));
    assert!(screen.contains("SELECTED TASK DETAIL"));
}

#[test]
fn supervisor_selection_changes_detail_and_disables_worker_commands() {
    let (_project, mut s) = state_with_worker_and_intervention();
    handle_key(&mut s, KeyCode::Up);
    assert!(s.selected_live_task().is_none());
    let buffer = render(&s, 120, 36);
    assert!(text(&buffer).contains("SUPERVISOR TASK DETAIL"));
    assert!(text(&buffer).contains("Windows desktop verification"));
    assert!(buffer
        .content
        .iter()
        .any(|c| c.symbol() == "▲" && c.fg == theme().warn && c.bg == theme().operate_selection));
    use command_catalog::Action as A;
    let commands = command_catalog::active(&s);
    for action in [A::StopTask, A::RetryTask, A::Diff] {
        assert!(commands.iter().any(|c| c.action == action && !c.enabled));
    }
    assert!(commands
        .iter()
        .any(|c| c.key == Some(KeyCode::Enter) && c.action == A::SupervisorReport && c.enabled));
    export_buffer("live-supervisor", &s, 120, 36);
    let compact = text(&render(&s, 48, 40));
    for label in [
        "Health", "Worker", "Task ID", "Status", "Tool", "Model", "Effort", "Age", "HB",
    ] {
        assert!(compact.contains(label), "{compact}");
    }
    handle_key(&mut s, KeyCode::Down);
    assert_eq!(s.selected_live_task().unwrap().task_id, "BOOT-DOC");
    assert!(text(&render(&s, 120, 36)).contains("SELECTED TASK DETAIL"));
    s.coordinator_snapshot.as_mut().unwrap().task_rows.clear();
    assert!(text(&render(&s, 120, 36)).contains("SUPERVISOR TASK DETAIL"));
    s.observations.latest.as_mut().unwrap().intervention = None;
    assert!(text(&render(&s, 120, 36)).contains("SELECTED TASK DETAIL"));
}

#[test]
fn stopped_run_is_visible_until_work_resumes_or_the_queue_is_resolved() {
    let (_project, mut state) = state_with_worker_and_intervention();
    state.coordinator_snapshot.as_mut().unwrap().active = 0;
    let screen = text(&render(&state, 160, 44));
    assert!(screen.contains("Stopped · blocked"));
    assert!(next_step_text(&state, 150).contains("BOOT-VERIFY"));
    state.coordinator_snapshot.as_mut().unwrap().blocked = 0;
    assert!(state.unresolved_run().is_none());
}

#[test]
fn blocked_check_is_selectable_without_counting_as_active_or_enabling_retry() {
    let (project, mut s) = fixture();
    navigation::Destination::Live.open(&mut s);
    let registry = serde_json::json!({"tasks":[{"id":"CHECKED","state":"blocked","task_runtime":{"resume_check":{
        "id":"example","category":"technical_condition","phase":"test","criterion":"Report for this commit",
        "last":{"status":"waiting","detail":"Publish staging report"},"checked_at":"2026-01-01T00:00:00Z"
    }}}]});
    macc_core::coordinator::state::coordinator_state_registry_save(
        project.path(),
        &Default::default(),
        &registry,
    )
    .unwrap();
    s.refresh_coordinator_snapshot();
    let snapshot = s.coordinator_snapshot.as_ref().unwrap();
    assert_eq!((snapshot.active, snapshot.blocked), (0, 1));
    assert_eq!(s.selected_live_task().unwrap().task_id, "CHECKED");
    assert!(text(&render(&s, 140, 40)).contains("Publish staging report"));
    let commands = command_catalog::active(&s);
    assert!(commands
        .iter()
        .any(|c| c.action == command_catalog::Action::RetryTask && !c.enabled));
}

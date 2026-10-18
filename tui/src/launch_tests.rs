use super::*;
#[test]
fn run_opens_a_review_and_cancel_starts_nothing() {
    let (_root, mut state) = crate::workspace_tests::fixture();
    crate::handle_key(&mut state, KeyCode::Char('r'));
    assert!(state.launch_dialog.is_some());
    assert_eq!(state.current_screen(), Screen::Home);
    crate::handle_key(&mut state, KeyCode::Esc);
    assert!(state.launch_dialog.is_none());
    assert!(state.launch_start_rx.is_none());
    assert!(!state.is_coordinator_running());
}
#[test]
fn required_reference_choices_are_visible_and_cannot_be_skipped() {
    let (_root, mut state) = crate::workspace_tests::fixture();
    state.launch_dialog = Some(LaunchDialog {
        review: LaunchReview {
            revision: "review".into(),
            task_source: "second.json (2/2)".into(),
            reference_branch: "main".into(),
            tools: vec!["future-tool".into()],
            max_parallel: 2,
            max_dispatch: 4,
            needs_branch_creation: true,
            dirty_reference: true,
            branch_sources: vec!["HEAD".into()],
            ..Default::default()
        },
        supervisor: true,
        create_from: None,
        allow_dirty: false,
        selected: 3,
        start_selected: true,
        error: None,
        scroll: 0,
    });
    for (w, h) in [(80, 24), (120, 36)] {
        crate::workspace_tests::export_buffer("launch-review", &state, w, h);
        let text = crate::workspace_tests::text(&crate::workspace_tests::render(&state, w, h));
        assert!(text.contains("Create missing reference"));
        assert!(text.contains("Allow dirty reference"));
        assert!(text.contains("[Esc] Cancel launch"));
    }
    crate::handle_key(&mut state, KeyCode::Enter);
    assert!(state.launch_dialog.as_ref().unwrap().error.is_some());
    assert!(state.launch_start_rx.is_none());
    state.launch_dialog.as_mut().unwrap().create_from = Some("HEAD".into());
    crate::handle_key(&mut state, KeyCode::Enter);
    assert!(state
        .launch_dialog
        .as_ref()
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .contains("dirty"));
    assert!(state.launch_start_rx.is_none());
}

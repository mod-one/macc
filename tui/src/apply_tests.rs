use super::*;
use macc_core::plan::{Action, ActionPlan};

#[test]
fn preview_does_not_fetch_while_apply_is_preparing_shared_sources() {
    let (_root, mut state) = crate::workspace_tests::fixture();
    let (tx, rx) = mpsc::channel();
    state.apply_runtime.preparing = Some(rx);
    state.open_preview();
    assert!(state
        .preview_error
        .as_deref()
        .unwrap()
        .contains("still preparing"));
    assert!(state.preview_ops.is_empty());
    drop(tx);
    state.poll_apply_preparation();
    assert!(!state.apply_preparing());
    assert!(state
        .apply_error
        .as_deref()
        .unwrap()
        .contains("stopped unexpectedly"));
}

fn state_with_plan(scope: Scope, count: usize) -> (tempfile::TempDir, AppState) {
    let (root, mut state) = crate::workspace_tests::fixture();
    let mut plan = ActionPlan::new();
    for i in 0..count {
        plan.add_action(Action::WriteFile {
            path: format!("output-{i:02}.txt"),
            content: b"approved content\n".to_vec(),
            scope,
        });
    }
    let operations = state
        .engine
        .plan_operations(state.project_paths.as_ref().unwrap(), &plan);
    let user_ops = if scope == Scope::User { count } else { 0 };
    state.apply_context = Some(ApplyContext {
        plan,
        operations,
        project_ops: count - user_ops,
        user_ops,
        backup_preview: ".macc/backups/<timestamp>".into(),
    });
    let context = state.apply_context.as_ref().unwrap();
    state.apply_runtime.planned_fingerprint = Some(
        macc_core::service::apply_review::fingerprint(&context.plan, &context.operations).unwrap(),
    );
    state.apply_runtime.planned_config = state.working_copy.clone();
    state.goto_screen(Screen::Apply);
    (root, state)
}
#[test]
fn project_apply_requires_a_separate_confirmation_and_cancel_does_not_write() {
    let (root, mut state) = state_with_plan(Scope::Project, 1);
    crate::handle_key(&mut state, KeyCode::Enter);
    assert!(state.apply_confirmation_open());
    assert!(!root.path().join("output-00.txt").exists());
    crate::handle_key(&mut state, KeyCode::Enter); // safe default: Cancel
    assert!(!state.apply_confirmation_open());
    assert!(!root.path().join("output-00.txt").exists());
    crate::handle_key(&mut state, KeyCode::Enter);
    crate::handle_key(&mut state, KeyCode::Right);
    crate::handle_key(&mut state, KeyCode::Enter);
    assert_eq!(
        std::fs::read_to_string(root.path().join("output-00.txt")).unwrap(),
        "approved content\n"
    );
    assert!(state.apply_feedback.is_some());
}

#[test]
fn o2_migration_is_visible_before_apply_confirmation() {
    let (_root, mut state) = state_with_plan(Scope::Project, 1);
    state.apply_context.as_mut().unwrap().plan.o2_activation =
        Some(macc_core::o2::activation::Activation {
            config_revision: "reviewed".into(),
            operation: "migrate sessions to SQLite and install O2".into(),
            source_sha256: Some("reviewed-source".into()),
        });
    state.attempt_apply();
    let rendered = crate::workspace_tests::text(&crate::workspace_tests::render(&state, 100, 30));
    assert!(rendered.contains("migrate sessions to SQLite"));
    assert!(rendered.contains("Migration backups are retained"));
}
#[test]
fn user_consent_and_configuration_changes_cannot_bypass_confirmation() {
    let (root, mut state) = state_with_plan(Scope::User, 1);
    state.attempt_apply();
    state.apply_key(KeyCode::Right);
    state.apply_key(KeyCode::Enter);
    assert!(state.apply_confirmation_open());
    assert!(state.apply_error.as_deref().unwrap().contains("typing YES"));
    assert!(!root.path().join("output-00.txt").exists());
    state.apply_key(KeyCode::Esc);
    state.attempt_apply();
    assert!(!state.apply_user_consent_granted);
    for c in "YES".chars() {
        state.apply_key(KeyCode::Char(c));
    }
    assert!(state.apply_user_consent_granted);
    state.working_copy.as_mut().unwrap().settings.offline = true;
    state.apply_key(KeyCode::Right);
    state.apply_key(KeyCode::Enter);
    assert!(state
        .apply_error
        .as_deref()
        .unwrap()
        .contains("Configuration changed"));
    assert!(!root.path().join("output-00.txt").exists());
}
#[test]
fn all_operations_remain_reachable_and_commands_visible_on_small_terminals() {
    let (_root, mut state) = state_with_plan(Scope::Project, 50);
    for _ in 0..49 {
        crate::handle_key(&mut state, KeyCode::Down);
    }
    for (w, h) in [(80, 24), (120, 36)] {
        crate::workspace_tests::export_buffer("apply", &state, w, h);
        let text = crate::workspace_tests::text(&crate::workspace_tests::render(&state, w, h));
        assert!(text.contains("output-49.txt"));
        assert!(text.contains("[F2] Commands"));
        assert!(!text.contains("and 40 more"));
        state.attempt_apply();
        crate::workspace_tests::export_buffer("apply-confirmation", &state, w, h);
        let modal = crate::workspace_tests::text(&crate::workspace_tests::render(&state, w, h));
        assert!(modal.contains("Confirm Apply"));
        assert!(modal.contains("[Esc] Cancel confirmation"));
        state.apply_key(KeyCode::Esc);
    }
}
#[test]
fn changed_files_require_another_preview_instead_of_overwriting_new_work() {
    let (root, mut state) = state_with_plan(Scope::Project, 1);
    std::fs::write(
        root.path().join("output-00.txt"),
        "work added after preview",
    )
    .unwrap();
    state.attempt_apply();
    state.apply_key(KeyCode::Right);
    state.apply_key(KeyCode::Enter);
    assert!(state
        .apply_error
        .as_deref()
        .unwrap()
        .contains("Files changed after preview"));
    assert_eq!(
        std::fs::read_to_string(root.path().join("output-00.txt")).unwrap(),
        "work added after preview"
    );
}

#[test]
fn failing_semantic_proof_cannot_bypass_the_apply_assistant() {
    let (_root, mut state) = state_with_plan(Scope::Project, 1);
    state.apply_runtime.assistant.open = true;
    state.apply_runtime.assistant.report = Some(macc_core::service::preparation::Report {
        current: None,
        live_output: None,
        revision: "review".into(),
        status: "waiting_for_input".into(),
        completed: vec![],
        error: None,
        question: Some(macc_core::service::preparation::Question {
            kind: "semantic_review".into(),
            message: "Review benchmark".into(),
            data: serde_json::json!({"approvable":false,"report":{"id":"failed"}}),
        }),
    });
    state.apply_key(KeyCode::Enter);
    assert!(state.apply_assistant_open());
    assert!(!state.apply_runtime.resume_after_prepare);
    let rendered = crate::workspace_tests::text(&crate::workspace_tests::render(&state, 100, 30));
    assert!(rendered.contains("Benchmark did not pass"));
    state.apply_key(KeyCode::Esc);
    assert!(!state.apply_assistant_open());
}

#[test]
fn apply_confirmation_scrolls_long_preparation_without_writing() {
    let (root, mut state) = state_with_plan(Scope::Project, 1);
    state.attempt_apply();
    state.apply_key(KeyCode::PageDown);
    assert_eq!(state.apply_runtime.confirm_scroll, 5);
    state.apply_key(KeyCode::PageUp);
    assert_eq!(state.apply_runtime.confirm_scroll, 0);
    assert!(!root.path().join("output-00.txt").exists());
}

#[test]
fn failed_apply_does_not_show_a_previous_ready_report_as_current() {
    let (_root, mut state) = state_with_plan(Scope::Project, 1);
    let path = state
        .project_paths
        .as_ref()
        .unwrap()
        .macc_dir
        .join("state/apply-preparation.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        r#"{"revision":"old","status":"ready","completed":[],"error":null,"question":null}"#,
    )
    .unwrap();
    state.complete_apply(Err("Configuration changed before execution".into()));
    assert!(state.apply_assistant_open());
    assert!(state.apply_runtime.assistant.report.is_none());
    let text = crate::workspace_tests::text(&crate::workspace_tests::render(&state, 100, 30));
    assert!(text.contains("Configuration changed before execution"));
}

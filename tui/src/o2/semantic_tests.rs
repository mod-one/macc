use super::*;
#[test]
fn semantic_policy_opt_in_requires_a_reviewed_preview() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let mut panel = Panel::default();
    panel.refresh(r).unwrap();
    panel.key(r, KeyCode::Char('5')).unwrap();
    panel.selected = panel
        .view
        .as_ref()
        .unwrap()
        .fields
        .iter()
        .position(|f| f.path == "memory.semantic_enabled")
        .unwrap();
    panel.key(r, KeyCode::Enter).unwrap();
    panel.key(r, KeyCode::Char('s')).unwrap();
    assert!(panel.message.contains("Press p"));
    assert!(!policy::load(r).unwrap().memory.semantic_enabled);
    panel.key(r, KeyCode::Char('p')).unwrap();
    panel.key(r, KeyCode::Char('s')).unwrap();
    assert!(policy::load(r).unwrap().memory.semantic_enabled);
}
#[test]
fn benchmark_form_prepares_an_exact_shared_action_and_does_not_execute_it() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let mut panel = Panel::default();
    panel.refresh(r).unwrap();
    panel.key(r, KeyCode::Char('B')).unwrap();
    assert!(panel.form.as_ref().unwrap().mutation);
    // The first guided step selects the number of repeated query/label rows.
    panel.key(r, KeyCode::Enter).unwrap();
    for v in [
        "first", "id-1", "second", "id-2", "third", "id-3", "fourth", "id-4", "3",
    ] {
        for c in v.chars() {
            panel.key(r, KeyCode::Char(c)).unwrap();
        }
        panel.key(r, KeyCode::Enter).unwrap();
    }
    let Some(Action::SemanticBenchmark { suite }) = panel.pending.as_ref() else {
        panic!("benchmark was executed instead of prepared")
    };
    assert_eq!(suite.cases.len(), 4);
    assert_eq!(suite.top_k, 3);
    assert!(panel.job.is_none());
    assert!(panel.mutation(KeyCode::Char('a')));
    panel.access = json!({"task_id":"changed-before-confirmation"});
    assert!(panel.key(r, KeyCode::Char('a')).is_err());
    assert!(panel.pending.is_none());
    assert!(panel.job.is_none());
}

#[test]
fn guided_benchmarks_support_four_to_one_hundred_rows_and_retain_bad_inputs() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    for count in [4, 5, 100] {
        let mut panel = Panel::default();
        panel.refresh(r).unwrap();
        panel.key(r, KeyCode::Char('B')).unwrap();
        for c in count.to_string().chars() {
            panel.key(r, KeyCode::Char(c)).unwrap();
        }
        panel.key(r, KeyCode::Enter).unwrap();
        assert_eq!(panel.form.as_ref().unwrap().labels.len(), count * 2 + 1);
        for i in 0..count {
            for value in [format!("query-{i}"), format!("memory-{i}")] {
                for c in value.chars() {
                    panel.key(r, KeyCode::Char(c)).unwrap();
                }
                panel.key(r, KeyCode::Enter).unwrap();
            }
        }
        panel.key(r, KeyCode::Char('0')).unwrap();
        assert!(panel.key(r, KeyCode::Enter).is_err());
        let form = panel.form.as_ref().unwrap();
        assert_eq!(form.values.len(), count * 2);
        assert_eq!(form.input, "0");
        panel.key(r, KeyCode::Backspace).unwrap();
        panel.key(r, KeyCode::Char('3')).unwrap();
        panel.key(r, KeyCode::Enter).unwrap();
        assert!(
            matches!(&panel.pending, Some(Action::SemanticBenchmark{suite}) if suite.cases.len()==count)
        );
        assert!(panel.job.is_none());
    }
    let mut panel = Panel::default();
    panel.key(r, KeyCode::Char('B')).unwrap();
    panel.key(r, KeyCode::Char('3')).unwrap();
    assert!(panel.key(r, KeyCode::Enter).is_err());
    assert_eq!(panel.form.as_ref().unwrap().name, "semantic_size");
    assert_eq!(panel.form.as_ref().unwrap().input, "3");
    panel.key(r, KeyCode::Esc).unwrap();
    assert!(panel.pending.is_none());
}

#[test]
fn legacy_semantic_report_requires_a_new_benchmark_but_can_be_revoked() {
    let t = tempfile::tempdir().unwrap();
    let mut panel = Panel::default();
    panel.refresh(t.path()).unwrap();
    panel.result(json!({"approvable":false,"checksum":"old","report":{"id":"old","passed":true}}));
    assert!(panel.key(t.path(), KeyCode::Char('G')).is_err());
    panel.key(t.path(), KeyCode::Char('X')).unwrap();
    assert!(matches!(
        &panel.pending,
        Some(Action::SemanticGate { enabled: false, .. })
    ));
}

#[test]
fn long_guided_suite_keeps_the_current_prompt_visible_in_a_small_terminal() {
    use ratatui::{backend::TestBackend, Terminal};
    let mut panel = Panel::default();
    panel.scroll = 500;
    panel.details = "Old report\n".repeat(100);
    panel
        .semantic_form("semantic_size", &["100".into()])
        .unwrap();
    assert_eq!(panel.scroll, 0);
    let form = panel.form.as_mut().unwrap();
    form.values = (0..192).map(|_| "completed input".into()).collect();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|f| panel.draw(f, Rect::new(0, 0, 80, 24), ""))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("Representative query 97 of 100"));
    assert!(text.contains("Shift+Tab previous"));
    assert!(!text.contains("Old report"));
}
#[test]
fn gate_approval_and_revocation_bind_the_inspected_report_and_current_scope() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let mut panel = Panel::default();
    panel.refresh(r).unwrap();
    panel.result(json!({"checksum":"exact-sha","report":{"id":"exact-report","passed":true}}));
    panel.key(r, KeyCode::Char('G')).unwrap();
    assert!(
        matches!(panel.pending,Some(Action::SemanticGate {ref report_id,ref checksum,enabled:true,confirmed:true}) if report_id=="exact-report" && checksum=="exact-sha")
    );
    panel.key(r, KeyCode::Char('X')).unwrap();
    assert!(matches!(
        panel.pending,
        Some(Action::SemanticGate { enabled: false, .. })
    ));
    panel.access = json!({"task_id":"other"});
    assert!(panel.key(r, KeyCode::Char('G')).is_err());
    assert!(panel.key(r, KeyCode::Char('a')).is_err());
    assert!(panel.pending.is_none());
    assert!(panel.job.is_none());
}

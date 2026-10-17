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
    assert!(panel.key(r, KeyCode::Char('s')).is_err());
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

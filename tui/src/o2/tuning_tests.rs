use super::*;
#[test]
fn trials_have_a_dedicated_guided_tab_and_mutations_require_confirmation() {
    let root = tempfile::tempdir().unwrap();
    let mut p = Panel::default();
    p.key(root.path(), KeyCode::Char('7')).unwrap();
    assert_eq!(p.tab, 6);
    // A missing SQLite authority is surfaced by the asynchronous inspector, not a crash.
    p.job = None;
    p.key(root.path(), KeyCode::Right).unwrap();
    assert_eq!(p.tuning_field, 1);
    p.key(root.path(), KeyCode::Char('n')).unwrap();
    assert_eq!(p.form.as_ref().unwrap().name, "tuning_create");
    assert!(p.mutation(KeyCode::Enter));
    let values = vec![
        "55".into(),
        "A, B".into(),
        "a".repeat(40),
        "protocol.md".into(),
        "sha".into(),
    ];
    p.form = None;
    p.tuning_form(root.path(), "tuning_create", &values)
        .unwrap();
    assert!(
        matches!(&p.pending,Some(Action::TuningCreate{plan,..}) if plan.field==macc_core::o2::tuning::FIELDS[1] && plan.cases.len()==2)
    );
    assert!(p.mutation(KeyCode::Char('a')));
    assert!(p.job.is_none());
    p.key(root.path(), KeyCode::Char('5')).unwrap();
    assert!(p.pending.is_none());
}
#[test]
fn policy_adoption_is_bound_to_the_exact_preview_and_expires_on_navigation() {
    let root = tempfile::tempdir().unwrap();
    let mut p = Panel {
        tab: 6,
        ..Default::default()
    };
    assert!(p.key(root.path(), KeyCode::Char('s')).is_err());
    p.result(
        json!({"report_id":"R","checksum":"sha","revision":"config-sha","before":{},"after":{}}),
    );
    p.key(root.path(), KeyCode::Char('s')).unwrap();
    assert!(matches!(&p.pending,Some(Action::TuningApply{revision,..}) if revision=="config-sha"));
    assert!(!p.mutation(KeyCode::Char('s')));
    p.tuning_preview = Some(json!({"report_id":"new-report"}));
    assert!(p.key(root.path(), KeyCode::Char('a')).is_err());
    assert!(p.pending.is_none());
}
#[test]
fn trial_quality_controls_and_policy_catalog_are_shared_and_failed_drafts_survive() {
    let root = tempfile::tempdir().unwrap();
    let mut p = Panel {
        tab: 6,
        ..Default::default()
    };
    p.refresh(root.path()).unwrap();
    p.job = None;
    for path in [
        "context_engine.tuning_enabled",
        "context_engine.tuning_min_pairs",
        "context_engine.tuning_alpha",
        "context_engine.tuning_min_cost_reduction",
        "context_engine.tuning_max_latency_regression",
    ] {
        assert!(p
            .view
            .as_ref()
            .unwrap()
            .fields
            .iter()
            .any(|f| f.path == path));
    }
    p.key(root.path(), KeyCode::Char('q')).unwrap();
    assert_eq!(p.form.as_ref().unwrap().labels.len(), 14);
    p.form = None;
    let v = [
        "trial",
        "task",
        "true",
        "true",
        "true",
        "0",
        "0",
        "true",
        "true",
        "true",
        "1",
        "0",
        "evidence.md",
        "sha",
    ]
    .map(str::to_string);
    p.tuning_form(root.path(), "tuning_observe", &v).unwrap();
    assert!(
        matches!(&p.pending,Some(Action::TuningObserve{observation,..}) if observation.candidate.max_finding_severity==1)
    );
    let mut invalid = v;
    invalid[2] = "yes".into();
    assert!(p
        .tuning_form(root.path(), "tuning_observe", &invalid)
        .is_err());
}

#[test]
fn trial_reports_do_not_replace_semantic_receipts_and_new_inspection_expires_adoption() {
    let mut p = Panel {
        tab: 6,
        ..Default::default()
    };
    p.semantic_report = Some(json!({"checksum":"semantic-sha","report":{"id":"semantic-report"}}));
    p.result(json!({"checksum":"trial-sha","report":{"id":"trial-report","passed":true}}));
    assert_eq!(
        p.semantic_report.as_ref().unwrap()["report"]["id"],
        "semantic-report"
    );
    p.tuning_preview = Some(json!({"report_id":"trial-report"}));
    p.pending = Some(Action::TuningApply {
        report_id: "trial-report".into(),
        checksum: "sha".into(),
        revision: "rev".into(),
        confirmed: true,
    });
    p.result(json!({"trials":[]}));
    assert!(p.tuning_preview.is_none());
    assert!(p.pending.is_none());
}

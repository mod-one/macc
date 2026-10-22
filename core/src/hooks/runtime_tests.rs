use super::tests::{external, import, root};
use super::*;
use crate::config::CanonicalConfig;
use serde_json::json;
#[test]
fn live_protocol_and_unclassified_commands_are_never_filtered() {
    let mut config = CanonicalConfig::default();
    let def = external();
    config.hooks = Some(HooksConfig {
        overrides: [(def.id.clone(), true)].into(),
        external: vec![review(def).unwrap()],
    });
    let ids = selected(&config, "fixture-check", &[]).unwrap();
    for raw in [
        "MACC_TASK_RESULT: success\nwarning: hi",
        "{\"jsonrpc\":\"2.0\",\"result\":42}",
    ] {
        assert_eq!(transform(&config, &ids, raw).unwrap(), raw);
    }
    assert!(selected(&config, "unclassified", &[]).unwrap().is_empty());
}
#[test]
fn evidence_collapses_only_exact_duplicates_and_keeps_distinct_failure_causes() {
    let (_temp, paths) = root();
    let one = r#"{"task_id":"one","type":"failed","error":"first cause"}"#;
    let two = r#"{"task_id":"one","type":"failed","error":"second cause"}"#;
    let raw = format!("{one}\n{one}\n{two}");
    let result = evidence(&paths.root, "coordinator-event-summarizer", &raw);
    assert!(result.contains("first cause"));
    assert!(result.contains("second cause"));
    assert_eq!(result.matches("first cause").count(), 1);
    assert!(result.contains("1 identical coordinator events collapsed"));
    assert_eq!(
        evidence(
            &paths.root,
            "coordinator-event-summarizer",
            "plain unstructured evidence"
        ),
        "plain unstructured evidence"
    );
}
#[test]
fn external_event_processor_uses_prompt_copies_without_touching_source_logs() {
    let (_temp, paths) = root();
    let mut def = external();
    def.event = Event::CoordinatorEvidence;
    def.handler = Handler::Rules {
        steps: vec![Step::Replace {
            pattern: "secret-value".into(),
            replacement: "REDACTED".into(),
        }],
    };
    let view = import(&paths, def);
    execute(
        &paths.root,
        Action::Set {
            id: "external-warning-filter".into(),
            enabled: Some(true),
            revision: view.revision,
        },
    )
    .unwrap();
    let source = r#"{"task_id":"one","type":"failed","detail":"secret-value"}"#;
    let log = paths.macc_dir.join("events.jsonl");
    std::fs::write(&log, source).unwrap();
    let output = evidence(&paths.root, "coordinator-event-summarizer", source);
    assert!(output.contains("REDACTED"));
    assert_eq!(std::fs::read_to_string(log).unwrap(), source);
}
#[test]
fn pipeline_runs_managed_rules_for_all_tool_ids_and_keeps_failed_output_and_raw_files() {
    use crate::skills_runner::{SkillDefinition, SkillRunRequest};
    let (_temp, paths) = root();
    let view = import(&paths, external());
    execute(
        &paths.root,
        Action::Set {
            id: "external-warning-filter".into(),
            enabled: Some(true),
            revision: view.revision,
        },
    )
    .unwrap();
    for tool in ["codex", "claude", "agy", "future-tool"] {
        for failed in [false, true] {
            let skill:SkillDefinition = serde_json::from_value(json!({"id":"fixture-check","title":"Fixture check","kind":"local_command","risk":"safe","steps":[{"run":if failed {"printf 'progress\\nwarning: preserve\\n'; exit 7"} else {"printf 'progress\\nwarning: preserve\\n'"}}]})).unwrap();
            let request = SkillRunRequest {
                skill_id: skill.id.clone(),
                tool_id: Some(tool.into()),
                cwd: paths.root.clone(),
                task_id: None,
                scope: None,
                inputs: Default::default(),
                dry_run: false,
                watch: false,
                yes: true,
            };
            let result = crate::context::pipeline::execute(&paths, &skill, &request, None).unwrap();
            assert!(result.stdout.contains("warning: preserve"));
            assert_eq!(result.stdout.contains("progress"), failed);
            assert_eq!(result.exit_code, Some(if failed { 7 } else { 0 }));
            let raw =
                std::fs::read_to_string(result.log_path.unwrap().with_extension("log")).unwrap();
            assert!(raw.contains("progress"));
            assert!(raw.contains("warning: preserve"));
        }
    }
}
#[test]
fn explicit_o2_reduce_obeys_hook_selection_and_external_rules() {
    let (_temp, paths) = root();
    let view = import(&paths, external());
    execute(
        &paths.root,
        Action::Set {
            id: "external-warning-filter".into(),
            enabled: Some(true),
            revision: view.revision,
        },
    )
    .unwrap();
    let reduced = crate::o2::output::reduce(
        &paths.root,
        "external-warning-filter",
        "progress\nwarning: keep",
    )
    .unwrap();
    assert!(reduced.summary.contains("warning: keep"));
    assert!(!reduced.summary.contains("progress"));
    let view = execute(&paths.root, Action::Status).unwrap();
    execute(
        &paths.root,
        Action::Set {
            id: "external-warning-filter".into(),
            enabled: Some(false),
            revision: view.revision,
        },
    )
    .unwrap();
    assert!(
        crate::o2::output::reduce(&paths.root, "external-warning-filter", "warning: keep").is_err()
    );
}
#[test]
fn empty_match_expansion_is_bounded_and_rejects_control_metadata() {
    let mut def = external();
    def.name = "unsafe\u{1b}[2J".into();
    assert!(review(def).is_err());
    let mut def = external();
    def.handler = Handler::Rules {
        steps: vec![Step::Replace {
            pattern: String::new(),
            replacement: "x".repeat(4096),
        }],
    };
    let mut config = CanonicalConfig::default();
    config.hooks = Some(HooksConfig {
        overrides: [(def.id.clone(), true)].into(),
        external: vec![review(def).unwrap()],
    });
    assert!(transform(
        &config,
        &["external-warning-filter".into()],
        &"a".repeat(3000)
    )
    .unwrap_err()
    .to_string()
    .contains("8 MiB"));
}

use super::*;
use crate::{
    engine::{Engine, MaccEngine},
    skills_runner::{SkillDefinition, SkillRunRequest},
    tool::{ToolAdapter, ToolRegistry},
};
use serde_json::json;
use std::sync::Arc;
fn skill(command: &str) -> SkillDefinition {
    serde_json::from_value(json!({"id":"build-check","title":"Build","kind":"local_command","risk":"safe","steps":[{"run":command}]})).unwrap()
}
fn request(root: &Path, tool: Option<&str>) -> SkillRunRequest {
    SkillRunRequest {
        skill_id: "build-check".into(),
        tool_id: tool.map(str::to_string),
        cwd: root.into(),
        task_id: None,
        scope: None,
        inputs: Default::default(),
        dry_run: false,
        watch: false,
        yes: true,
    }
}
#[test]
fn normal_execution_reduces_both_streams_preserves_raw_evidence_and_failure_status() {
    let temp = tests::root();
    let root = temp.path();
    let view = policy::inspect(root).unwrap();
    let mut p = view.policy;
    p.context_engine.pull_limit_tokens = 100;
    policy::update(root, &view.revision, Some(p)).unwrap();
    let engine = MaccEngine::new(ToolRegistry::new());
    let result = engine
        .run_skill(
            &crate::ProjectPaths::from_root(root),
            &skill("printf 'progress\\n%.0s' {1..200}; printf 'ERROR retained\\n' >&2; exit 7"),
            &request(root, None),
        )
        .unwrap();
    assert_eq!(result.exit_code, Some(7));
    assert_eq!(result.status, "failed");
    assert!(result.stderr.contains("ERROR retained"));
    assert!(
        crate::context::estimate_tokens(&result.stdout)
            + crate::context::estimate_tokens(&result.stderr)
            <= 100
    );
    assert!(result.summary.is_some());
    let raw = result.log_path.unwrap().with_extension("log");
    let full = std::fs::read_to_string(&raw).unwrap();
    assert!(full.contains("progress"));
    assert!(full.contains("ERROR retained"));
    assert!(result.stdout.contains("Raw artifact:"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(raw).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
struct Hook;
impl ToolAdapter for Hook {
    fn id(&self) -> String {
        "future-fixture".into()
    }
    fn plan(&self, _: &crate::resolve::PlanningContext) -> Result<crate::plan::ActionPlan> {
        Err(error("Planning is not used by this test"))
    }
    fn transform_tool_output(
        &self,
        ctx: &crate::context::pipeline::ToolOutput<'_>,
    ) -> Result<Option<String>> {
        assert_eq!(ctx.skill.id, "build-check");
        Ok(Some("ERROR adapter transform retained".into()))
    }
}
#[test]
fn execution_uses_adapter_transform_and_keeps_machine_protocol_opaque() {
    let temp = tests::root();
    let root = temp.path();
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(Hook));
    let engine = MaccEngine::new(registry);
    let paths = crate::ProjectPaths::from_root(root);
    let result = engine
        .run_skill(
            &paths,
            &skill("printf 'plain progress'"),
            &request(root, Some("future-fixture")),
        )
        .unwrap();
    assert!(result.stdout.contains("adapter transform"));
    assert!(result
        .summary
        .unwrap()
        .bundles_applied
        .iter()
        .any(|s| s == "adapter_output_hook"));
    let result = engine
        .run_skill(
            &paths,
            &skill(
                "printf '%s\\n' 'MACC_TOOL_LIMIT: cooldown' 'MACC_TASK_RESULT: precondition_unmet'",
            ),
            &request(root, Some("future-fixture")),
        )
        .unwrap();
    assert_eq!(
        result.stdout,
        "MACC_TOOL_LIMIT: cooldown\nMACC_TASK_RESULT: precondition_unmet\n"
    );
    assert!(result.summary.is_none());
    let result = engine
        .run_skill(
            &paths,
            &skill("printf '%s\\n' '{\"usage\":{\"input_tokens\":42},\"session_id\":\"exact\"}'"),
            &request(root, Some("future-fixture")),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result.stdout).unwrap()["session_id"],
        "exact"
    );
    assert!(result.summary.is_none());
}
#[test]
fn disabling_output_reduction_preserves_existing_execution_behavior() {
    let temp = tests::root();
    let root = temp.path();
    let view = policy::inspect(root).unwrap();
    let mut p = view.policy;
    p.context_engine.output_reduction = false;
    policy::update(root, &view.revision, Some(p)).unwrap();
    let result = MaccEngine::new(ToolRegistry::new())
        .run_skill(
            &crate::ProjectPaths::from_root(root),
            &skill("printf 'unchanged plain output'"),
            &request(root, None),
        )
        .unwrap();
    assert_eq!(result.stdout, "unchanged plain output");
    assert!(result.summary.is_none());
}

#[test]
fn unclassified_data_and_failed_diagnostics_survive_default_and_explicit_reducers() {
    let temp = tests::root();
    let root = temp.path();
    let engine = MaccEngine::new(ToolRegistry::new());
    let paths = crate::ProjectPaths::from_root(root);
    let result = engine
        .run_skill(
            &paths,
            &skill("printf 'contract version: 42'"),
            &request(root, None),
        )
        .unwrap();
    assert!(result.stdout.contains("contract version: 42"));
    assert!(!result.stdout.contains("No errors"));
    let mut config = crate::load_canonical_config(&paths.config_path).unwrap();
    config.context = Some(
        serde_json::from_value(
            json!({"summarization":{"enabled":true,"default_bundles":["log-grep-error-first"]}}),
        )
        .unwrap(),
    );
    std::fs::write(&paths.config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
    for diagnostic in [
        "permission denied accessing contract.rs",
        "connection refused",
        "command not found",
    ] {
        let command = format!("printf '{diagnostic}' >&2; exit 7");
        let result = engine
            .run_skill(&paths, &skill(&command), &request(root, None))
            .unwrap();
        assert_eq!(result.exit_code, Some(7));
        assert_eq!(result.status, "failed");
        assert!(result.stderr.contains(diagnostic), "{}", result.stderr);
        assert!(!result.stderr.contains("No errors"));
        assert!(result
            .summary
            .unwrap()
            .bundles_applied
            .contains(&"failure_output_preserved".into()));
    }
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(Hook));
    let result = MaccEngine::new(registry)
        .run_skill(
            &paths,
            &skill("printf 'permission denied' >&2; exit 7"),
            &request(root, Some("future-fixture")),
        )
        .unwrap();
    assert!(result.stderr.contains("permission denied"));
    assert!(!result.stderr.contains("adapter transform"));
}

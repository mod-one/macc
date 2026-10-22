use super::*;
use serde_json::json;

#[test]
fn omitted_and_partial_sections_inherit_local_defaults_but_explicit_opt_out_wins() {
    for raw in ["", "context_engine: {}\ncode_intelligence: {}\n"] {
        let config = serde_yaml::from_str::<crate::config::CanonicalConfig>(&format!(
            "tools:\n  enabled: []\n{raw}"
        ))
        .unwrap();
        let p = from_config(&config);
        p.validate().unwrap();
        assert!(p.context_engine.enabled && p.context_engine.output_reduction);
        assert!(p.code_intelligence.enabled && p.code_intelligence.native_map_enabled);
        assert_eq!(p.code_intelligence.routing, "native_only");
        assert!(!p.memory.enabled && !p.memory.mcp_enabled && !p.memory.semantic_enabled);
        assert!(!p.adaptive_compaction.enabled && !p.context_engine.tuning_enabled);
        assert!(!p.code_intelligence.serena_enabled && !p.code_intelligence.graphify_enabled);
    }
    let config = serde_yaml::from_str::<crate::config::CanonicalConfig>(
        "tools:\n  enabled: []\ncontext_engine:\n  enabled: false\n  output_reduction: false\ncode_intelligence:\n  enabled: false\n  native_map_enabled: false\n",
    ).unwrap();
    let p = from_config(&config);
    p.validate().unwrap();
    assert!(!p.context_engine.enabled && !p.context_engine.output_reduction);
    assert!(!p.code_intelligence.enabled && !p.code_intelligence.native_map_enabled);
    assert!(!super::super::activation::requested(&config));
}

#[test]
fn default_context_without_memory_never_injects_previously_approved_facts() {
    let temp = super::super::tests::root();
    let root = temp.path();
    let fact = super::super::memory::propose(root, &super::super::tests::proposal(root, Some("A")))
        .unwrap();
    super::super::tests::activate(root, &fact, Some("A"));
    let view = inspect(root).unwrap();
    let mut policy = view.policy;
    policy.memory.enabled = false;
    super::update(root, &view.revision, Some(policy)).unwrap();
    std::fs::write(root.join("worktree.prd.json"), json!({"tasks":[{"id":"A","acceptance_criteria":["preserve mandatory contract"],"explicit_sources":["contract.rs"]}]}).to_string()).unwrap();
    let identity = crate::coordinator::session_identity::identity(
        root,
        root,
        "A",
        "performer",
        "future",
        &json!({"id":"future","performer":{"command":"fake","session":{"enabled":true,"scope":"task_role_tool","create":{"command":"fake","args":["new","{session_id}"]}}}}),
        &crate::coordinator::session_identity::SessionRouting {
            tier: "".into(),
            mode: "auto".into(),
        },
    )
    .unwrap();
    let prepared = crate::session_store::checkpoints::prepare(
        root,
        &crate::session_store::checkpoints::ContextRequest {
            identity,
            phase: "dev".into(),
        },
    )
    .unwrap();
    assert!(prepared.prompt.contains("preserve mandatory contract"));
    assert!(!prepared.prompt.contains("public contract must"));
    assert!(super::super::context::list(root).unwrap().is_empty());
}

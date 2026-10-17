use super::waiting_for_tools_native;
use crate::coordinator::{rate_limit::ToolThrottleState, runtime::CoordinatorRunState};
use std::collections::BTreeMap;

#[test]
fn quota_wait_requires_a_ready_task_and_no_available_fallback() {
    let root = tempfile::tempdir().unwrap();
    let mut config = crate::config::CanonicalConfig::default();
    config.tools.enabled = vec!["codex".into(), "claude".into()];
    let mut state = CoordinatorRunState::new();
    let until = chrono::Utc::now().timestamp() as u64 + 3600;
    let throttle = |tool: &str| ToolThrottleState {
        tool_id: tool.into(),
        throttled_until: until,
        ..Default::default()
    };
    state
        .throttle_registry
        .insert("codex".into(), throttle("codex"));
    let registry = serde_json::json!({"tasks":[{"id":"A","state":"todo","tool":"codex"}],"external_merged_task_ids":[]});
    crate::coordinator::state::coordinator_state_registry_save(
        root.path(),
        &BTreeMap::new(),
        &registry,
    )
    .unwrap();
    let env = Default::default();
    assert!(!waiting_for_tools_native(
        root.path(),
        &config,
        None,
        &env,
        &state
    ));
    state
        .throttle_registry
        .insert("claude".into(), throttle("claude"));
    assert!(waiting_for_tools_native(
        root.path(),
        &config,
        None,
        &env,
        &state
    ));
    let registry =
        serde_json::json!({"tasks":[{"id":"A","state":"blocked"}],"external_merged_task_ids":[]});
    crate::coordinator::state::coordinator_state_registry_save(
        root.path(),
        &BTreeMap::new(),
        &registry,
    )
    .unwrap();
    assert!(!waiting_for_tools_native(
        root.path(),
        &config,
        None,
        &env,
        &state
    ));
}

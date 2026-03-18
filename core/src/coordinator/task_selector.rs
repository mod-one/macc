use crate::coordinator::model::{Task, TaskRegistry};
use crate::coordinator::rate_limit::{is_task_delayed, is_tool_throttled, ToolThrottleRegistry};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskSelectorConfig {
    pub enabled_tools: Vec<String>,
    pub tool_priority: Vec<String>,
    pub max_parallel_per_tool: HashMap<String, usize>,
    pub tool_specializations: HashMap<String, Vec<String>>,
    pub max_parallel: usize,
    pub default_tool: String,
    pub default_base_branch: String,
    /// Current wall-clock timestamp in ISO 8601 / RFC 3339 format (e.g.
    /// `"2026-03-18T12:00:00Z"`).  When set, tasks whose `delayed_until` is
    /// still in the future are excluded from dispatch.  An empty string
    /// disables the delay filter (all tasks are eligible).
    pub now: String,
    /// Per-tool throttle state used to filter out currently rate-limited tools.
    /// Empty map disables throttle filtering.
    pub throttle_registry: ToolThrottleRegistry,
    /// When `true`, `pick_tool()` will skip throttled tools and select the
    /// next available tool in priority order (fallback routing).
    pub rate_limit_fallback_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedTask {
    pub id: String,
    pub title: String,
    pub tool: String,
    pub base_branch: String,
    /// `true` when the selected tool differs from the primary (highest-priority)
    /// tool due to throttle filtering (RL-ROUTE-005 fallback routing).
    pub is_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchBlockReason {
    ActivePriorityZero { task_id: String },
    ReadyPriorityZeroBlocked { task_id: String },
}

pub fn dispatch_block_reason(
    registry: &Value,
    config: &TaskSelectorConfig,
) -> Option<DispatchBlockReason> {
    let typed = TaskRegistry::from_value(registry).ok()?;
    dispatch_block_reason_typed(&typed, config)
}

pub fn dispatch_block_reason_typed(
    registry: &TaskRegistry,
    config: &TaskSelectorConfig,
) -> Option<DispatchBlockReason> {
    let active_tasks: Vec<&Task> = registry
        .tasks
        .iter()
        .filter(|task| task.is_active())
        .collect();

    if let Some(task_id) = active_tasks
        .iter()
        .find(|task| task.priority_rank() == 0)
        .map(|task| task.id.clone())
    {
        return Some(DispatchBlockReason::ActivePriorityZero { task_id });
    }

    if active_tasks.is_empty() {
        return None;
    }

    let merged_ids: HashSet<String> = registry
        .tasks
        .iter()
        .filter(|task| task.is_merged())
        .map(|task| task.id.clone())
        .collect();
    let resource_locks = &registry.resource_locks;

    for task in &registry.tasks {
        if task.workflow_state() != Some(crate::coordinator::WorkflowState::Todo) {
            continue;
        }
        if task.has_worktree_attached() {
            continue;
        }
        if task.id.is_empty() || task.priority_rank() != 0 {
            continue;
        }
        if !dependencies_ready(task, &merged_ids) {
            continue;
        }
        if !resources_available(task, resource_locks) {
            continue;
        }
        if pick_tool(task, config, &HashMap::new()).is_none() {
            continue;
        }
        // (throttle filtering is intentionally skipped in the block-reason
        // check — we need to know whether the task *could* dispatch, not
        // whether it can right now.)
        return Some(DispatchBlockReason::ReadyPriorityZeroBlocked {
            task_id: task.id.clone(),
        });
    }

    None
}

pub fn select_next_ready_task(
    registry: &Value,
    config: &TaskSelectorConfig,
) -> Option<SelectedTask> {
    let typed = TaskRegistry::from_value(registry).ok()?;
    select_next_ready_task_typed(&typed, config)
}

pub fn select_next_ready_task_typed(
    registry: &TaskRegistry,
    config: &TaskSelectorConfig,
) -> Option<SelectedTask> {
    let active_tasks: Vec<&Task> = registry
        .tasks
        .iter()
        .filter(|task| task.is_active())
        .collect();
    if dispatch_block_reason_typed(registry, config).is_some() {
        return None;
    }
    if config.max_parallel > 0 && active_tasks.len() >= config.max_parallel {
        return None;
    }

    let merged_ids: HashSet<String> = registry
        .tasks
        .iter()
        .filter(|task| task.is_merged())
        .map(|task| task.id.clone())
        .collect();

    let mut active_by_tool: HashMap<String, usize> = HashMap::new();
    for task in active_tasks {
        if let Some(tool) = task.task_tool() {
            *active_by_tool.entry(tool.to_string()).or_insert(0) += 1;
        }
    }

    let resource_locks = &registry.resource_locks;
    let mut candidates: Vec<(i32, String, String, SelectedTask)> = Vec::new();

    for task in &registry.tasks {
        if task.workflow_state() != Some(crate::coordinator::WorkflowState::Todo) {
            continue;
        }
        if task.has_worktree_attached() {
            continue;
        }
        if task.id.is_empty() {
            continue;
        }
        if !dependencies_ready(task, &merged_ids) {
            continue;
        }
        if !resources_available(task, resource_locks) {
            continue;
        }
        if is_task_delayed(task, &config.now) {
            continue;
        }

        let Some((tool, is_fallback)) = pick_tool(task, config, &active_by_tool) else {
            continue;
        };

        candidates.push((
            task.priority_rank(),
            task.category().unwrap_or("zzz").to_string(),
            task.id.clone(),
            SelectedTask {
                id: task.id.clone(),
                title: task.title.clone().unwrap_or_default(),
                tool,
                base_branch: task.base_branch(&config.default_base_branch),
                is_fallback,
            },
        ));
    }

    candidates.sort_by(|a, b| (&a.0, &a.1, &a.2).cmp(&(&b.0, &b.1, &b.2)));
    candidates
        .into_iter()
        .next()
        .map(|(_, _, _, selected)| selected)
}

fn dependencies_ready(task: &Task, merged_ids: &HashSet<String>) -> bool {
    task.dependency_ids()
        .iter()
        .all(|dependency| merged_ids.contains(dependency))
}

fn resources_available(
    task: &Task,
    locks: &BTreeMap<String, crate::coordinator::model::ResourceLock>,
) -> bool {
    task.exclusive_resources.iter().all(|resource| {
        if resource.is_empty() {
            return true;
        }
        match locks.get(resource) {
            Some(lock) => lock.task_id.is_empty() || lock.task_id == task.id,
            None => true,
        }
    })
}

/// Returns `(tool_id, is_fallback)`. `is_fallback` is `true` when the
/// selected tool differs from the highest-priority candidate due to
/// throttle filtering (RL-ROUTE-005).
fn pick_tool(
    task: &Task,
    config: &TaskSelectorConfig,
    active_by_tool: &HashMap<String, usize>,
) -> Option<(String, bool)> {
    let preference = preference_list(task, config);
    let fallback = fallback_pool(task, config, &preference);

    let mut combined = Vec::new();
    combined.extend(preference.iter().cloned());
    combined.extend(fallback);

    let mut uniq = Vec::new();
    let mut seen = HashSet::new();
    for tool in combined {
        if seen.insert(tool.clone()) {
            uniq.push(tool);
        }
    }

    let enabled_set: Option<HashSet<String>> = if config.enabled_tools.is_empty() {
        None
    } else {
        Some(config.enabled_tools.iter().cloned().collect())
    };

    let pref_rank: BTreeMap<String, usize> = preference
        .iter()
        .enumerate()
        .map(|(index, tool)| (tool.clone(), index))
        .collect();

    let mut candidates: Vec<(usize, usize, String)> = Vec::new();
    for tool in uniq {
        if let Some(enabled) = &enabled_set {
            if !enabled.contains(&tool) {
                continue;
            }
        }
        if let Some(capacity) = config.max_parallel_per_tool.get(&tool) {
            let current = *active_by_tool.get(&tool).unwrap_or(&0);
            if current >= *capacity {
                continue;
            }
        }
        let rank = *pref_rank.get(&tool).unwrap_or(&999);
        let load = *active_by_tool.get(&tool).unwrap_or(&0);
        candidates.push((rank, load, tool));
    }

    candidates.sort_by(|a, b| (&a.0, &a.1, &a.2).cmp(&(&b.0, &b.1, &b.2)));

    // Identify the primary (highest-priority) tool before throttle filtering.
    let primary_tool = candidates.first().map(|(_, _, tool)| tool.clone());

    // RL-ROUTE-005: throttle filtering — skip throttled tools and fall back
    // to the next available one.  Disabled for review/fix phases (idempotency
    // guard: the task is mid-flight and must not switch tools).
    let apply_throttle_filter = config.rate_limit_fallback_enabled
        && !config.throttle_registry.is_empty()
        && !matches!(
            task.task_runtime.current_phase.as_deref(),
            Some("review") | Some("fix")
        );

    let selected = if apply_throttle_filter {
        candidates
            .into_iter()
            .find(|(_, _, tool)| !is_tool_throttled(&config.throttle_registry, tool, &config.now))
            .map(|(_, _, tool)| tool)
    } else {
        candidates.into_iter().next().map(|(_, _, tool)| tool)
    };

    let is_fallback = selected.is_some() && selected != primary_tool;
    selected.map(|tool| (tool, is_fallback))
}

fn preference_list(task: &Task, config: &TaskSelectorConfig) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(category) = task.category() {
        if let Some(tools) = config.tool_specializations.get(category) {
            out.extend(tools.iter().cloned());
        }
    }
    if out.is_empty() {
        if let Some(task_tool) = task.task_tool() {
            out.push(task_tool.to_string());
        } else if !config.tool_priority.is_empty() {
            out.extend(config.tool_priority.iter().cloned());
        }
    }
    dedup_and_clean(out)
}

fn fallback_pool(task: &Task, config: &TaskSelectorConfig, preference: &[String]) -> Vec<String> {
    if !config.enabled_tools.is_empty() {
        return config.enabled_tools.clone();
    }

    let mut out = Vec::new();
    out.extend(preference.iter().cloned());
    out.extend(config.tool_priority.iter().cloned());
    if let Some(task_tool) = task.task_tool() {
        out.push(task_tool.to_string());
    }
    out.push(config.default_tool.clone());

    let mut specialization_tools = BTreeSet::new();
    for tools in config.tool_specializations.values() {
        for tool in tools {
            specialization_tools.insert(tool.clone());
        }
    }
    out.extend(specialization_tools);

    dedup_and_clean(out)
}

fn dedup_and_clean(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.clone()) {
            out.push(trimmed);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn picks_highest_priority_ready_task() {
        let registry = json!({
          "tasks": [
            {"id":"B","title":"B","state":"todo","priority":"2","dependencies":[],"exclusive_resources":[]},
            {"id":"A","title":"A","state":"todo","priority":"1","dependencies":[],"exclusive_resources":[]}
          ],
          "resource_locks": {}
        });
        let cfg = TaskSelectorConfig {
            default_tool: "codex".into(),
            default_base_branch: "master".into(),
            max_parallel: 3,
            ..TaskSelectorConfig::default()
        };
        let selected = select_next_ready_task(&registry, &cfg).expect("selected task");
        assert_eq!(selected.id, "A");
    }

    #[test]
    fn respects_dependencies_and_resource_locks() {
        let registry = json!({
          "tasks": [
            {"id":"DEP","title":"dep","state":"todo","priority":"1","dependencies":["X"],"exclusive_resources":[]},
            {"id":"OK","title":"ok","state":"todo","priority":"2","dependencies":[],"exclusive_resources":["res-a"]},
            {"id":"X","title":"x","state":"merged","priority":"1","dependencies":[],"exclusive_resources":[]}
          ],
          "resource_locks": {
            "res-a": {"task_id":"OTHER"}
          }
        });
        let cfg = TaskSelectorConfig {
            default_tool: "codex".into(),
            default_base_branch: "master".into(),
            max_parallel: 3,
            ..TaskSelectorConfig::default()
        };
        let selected = select_next_ready_task(&registry, &cfg).expect("selected task");
        assert_eq!(selected.id, "DEP");
    }

    #[test]
    fn active_priority_zero_blocks_new_dispatch() {
        let registry = json!({
          "tasks": [
            {"id":"P0","title":"p0","state":"claimed","priority":"0","dependencies":[],"exclusive_resources":[]},
            {"id":"A","title":"a","state":"todo","priority":"1","dependencies":[],"exclusive_resources":[]}
          ],
          "resource_locks": {}
        });
        let cfg = TaskSelectorConfig {
            default_tool: "codex".into(),
            default_base_branch: "master".into(),
            max_parallel: 3,
            ..TaskSelectorConfig::default()
        };
        assert_eq!(
            dispatch_block_reason(&registry, &cfg),
            Some(DispatchBlockReason::ActivePriorityZero {
                task_id: "P0".into()
            })
        );
        assert_eq!(select_next_ready_task(&registry, &cfg), None);
    }

    #[test]
    fn ready_priority_zero_waits_for_exclusive_slot() {
        let registry = json!({
          "tasks": [
            {"id":"RUN","title":"run","state":"claimed","priority":"2","dependencies":[],"exclusive_resources":[]},
            {"id":"P0","title":"p0","state":"todo","priority":"0","dependencies":[],"exclusive_resources":[]},
            {"id":"LATER","title":"later","state":"todo","priority":"1","dependencies":[],"exclusive_resources":[]}
          ],
          "resource_locks": {}
        });
        let cfg = TaskSelectorConfig {
            default_tool: "codex".into(),
            default_base_branch: "master".into(),
            max_parallel: 3,
            ..TaskSelectorConfig::default()
        };
        assert_eq!(
            dispatch_block_reason(&registry, &cfg),
            Some(DispatchBlockReason::ReadyPriorityZeroBlocked {
                task_id: "P0".into()
            })
        );
        assert_eq!(select_next_ready_task(&registry, &cfg), None);
    }

    // ── RL-DISPATCH-004: delayed_until filtering ──────────────────────

    fn cfg_with_now(now: &str) -> TaskSelectorConfig {
        TaskSelectorConfig {
            default_tool: "worker".into(),
            default_base_branch: "main".into(),
            max_parallel: 3,
            now: now.to_string(),
            ..TaskSelectorConfig::default()
        }
    }

    #[test]
    fn task_with_future_delayed_until_is_skipped() {
        let registry = json!({
          "tasks": [
            {
              "id": "DELAYED",
              "title": "rate-limited task",
              "state": "todo",
              "priority": "1",
              "dependencies": [],
              "exclusive_resources": [],
              "task_runtime": { "delayed_until": "2026-03-18T12:05:00Z" }
            }
          ],
          "resource_locks": {}
        });
        let cfg = cfg_with_now("2026-03-18T12:00:00Z");
        assert_eq!(
            select_next_ready_task(&registry, &cfg),
            None,
            "delayed task must not be selected before delayed_until"
        );
    }

    #[test]
    fn task_with_past_delayed_until_is_eligible() {
        let registry = json!({
          "tasks": [
            {
              "id": "READY",
              "title": "previously rate-limited",
              "state": "todo",
              "priority": "1",
              "dependencies": [],
              "exclusive_resources": [],
              "task_runtime": { "delayed_until": "2026-03-18T11:55:00Z" }
            }
          ],
          "resource_locks": {}
        });
        let cfg = cfg_with_now("2026-03-18T12:00:00Z");
        let selected = select_next_ready_task(&registry, &cfg).expect("should select eligible task");
        assert_eq!(selected.id, "READY");
    }

    #[test]
    fn task_without_delayed_until_is_always_eligible() {
        let registry = json!({
          "tasks": [
            {
              "id": "NODLY",
              "title": "no delay",
              "state": "todo",
              "priority": "1",
              "dependencies": [],
              "exclusive_resources": []
            }
          ],
          "resource_locks": {}
        });
        let cfg = cfg_with_now("2026-03-18T12:00:00Z");
        let selected = select_next_ready_task(&registry, &cfg).expect("should select undelayed task");
        assert_eq!(selected.id, "NODLY");
    }

    #[test]
    fn delayed_task_skipped_but_undelayed_sibling_selected() {
        let registry = json!({
          "tasks": [
            {
              "id": "DELAYED",
              "title": "delayed",
              "state": "todo",
              "priority": "1",
              "dependencies": [],
              "exclusive_resources": [],
              "task_runtime": { "delayed_until": "2026-03-18T12:30:00Z" }
            },
            {
              "id": "FREE",
              "title": "free",
              "state": "todo",
              "priority": "1",
              "dependencies": [],
              "exclusive_resources": []
            }
          ],
          "resource_locks": {}
        });
        let cfg = cfg_with_now("2026-03-18T12:00:00Z");
        let selected = select_next_ready_task(&registry, &cfg).expect("should select non-delayed task");
        assert_eq!(selected.id, "FREE");
    }

    #[test]
    fn empty_now_disables_delay_filter() {
        // When config.now is empty, is_task_delayed always returns false.
        let registry = json!({
          "tasks": [
            {
              "id": "T1",
              "title": "t1",
              "state": "todo",
              "priority": "1",
              "dependencies": [],
              "exclusive_resources": [],
              "task_runtime": { "delayed_until": "9999-12-31T23:59:59Z" }
            }
          ],
          "resource_locks": {}
        });
        let cfg = cfg_with_now(""); // empty disables filter
        let selected = select_next_ready_task(&registry, &cfg).expect("filter disabled → task selected");
        assert_eq!(selected.id, "T1");
    }

    #[test]
    fn non_priority_zero_tasks_can_still_dispatch_in_parallel() {
        let registry = json!({
          "tasks": [
            {"id":"RUN","title":"run","state":"claimed","priority":"2","dependencies":[],"exclusive_resources":[]},
            {"id":"NEXT","title":"next","state":"todo","priority":"1","dependencies":[],"exclusive_resources":[]}
          ],
          "resource_locks": {}
        });
        let cfg = TaskSelectorConfig {
            default_tool: "codex".into(),
            default_base_branch: "master".into(),
            max_parallel: 3,
            ..TaskSelectorConfig::default()
        };
        assert_eq!(dispatch_block_reason(&registry, &cfg), None);
        let selected = select_next_ready_task(&registry, &cfg).expect("selected task");
        assert_eq!(selected.id, "NEXT");
    }

    // ── RL-ROUTE-005: tool throttle fallback routing ──────────────────

    fn throttle_registry_for(tool: &str, throttled_until_epoch: u64) -> ToolThrottleRegistry {
        let mut reg = ToolThrottleRegistry::default();
        reg.insert(
            tool.to_string(),
            crate::coordinator::rate_limit::ToolThrottleState {
                tool_id: tool.to_string(),
                throttled_until: throttled_until_epoch,
                consecutive_429_count: 1,
                backoff_seconds: 30,
                last_rate_limit_info: None,
            },
        );
        reg
    }

    fn epoch_far_future() -> u64 {
        // 2099-01-01T00:00:00Z
        4_070_908_800
    }

    #[test]
    fn throttled_primary_tool_falls_back_to_next_priority() {
        let registry = json!({
          "tasks": [
            {"id":"T1","title":"t1","state":"todo","priority":"1","dependencies":[],"exclusive_resources":[]}
          ],
          "resource_locks": {}
        });
        let cfg = TaskSelectorConfig {
            default_tool: "fallback".into(),
            default_base_branch: "main".into(),
            max_parallel: 3,
            tool_priority: vec!["primary".into(), "fallback".into()],
            enabled_tools: vec!["primary".into(), "fallback".into()],
            throttle_registry: throttle_registry_for("primary", epoch_far_future()),
            rate_limit_fallback_enabled: true,
            now: "2026-03-18T12:00:00Z".into(),
            ..TaskSelectorConfig::default()
        };
        let selected = select_next_ready_task(&registry, &cfg).expect("fallback tool selected");
        assert_eq!(selected.tool, "fallback");
        assert!(selected.is_fallback, "is_fallback must be true");
    }

    #[test]
    fn expired_throttle_does_not_trigger_fallback() {
        let registry = json!({
          "tasks": [
            {"id":"T1","title":"t1","state":"todo","priority":"1","dependencies":[],"exclusive_resources":[]}
          ],
          "resource_locks": {}
        });
        // throttled_until is in the past (epoch 1)
        let cfg = TaskSelectorConfig {
            default_tool: "primary".into(),
            default_base_branch: "main".into(),
            max_parallel: 3,
            tool_priority: vec!["primary".into(), "fallback".into()],
            enabled_tools: vec!["primary".into(), "fallback".into()],
            throttle_registry: throttle_registry_for("primary", 1),
            rate_limit_fallback_enabled: true,
            now: "2026-03-18T12:00:00Z".into(),
            ..TaskSelectorConfig::default()
        };
        let selected = select_next_ready_task(&registry, &cfg).expect("primary selected after expiry");
        assert_eq!(selected.tool, "primary");
        assert!(!selected.is_fallback, "is_fallback must be false when throttle expired");
    }

    #[test]
    fn fallback_disabled_by_config_uses_primary_even_when_throttled() {
        let registry = json!({
          "tasks": [
            {"id":"T1","title":"t1","state":"todo","priority":"1","dependencies":[],"exclusive_resources":[]}
          ],
          "resource_locks": {}
        });
        let cfg = TaskSelectorConfig {
            default_tool: "fallback".into(),
            default_base_branch: "main".into(),
            max_parallel: 3,
            tool_priority: vec!["primary".into(), "fallback".into()],
            enabled_tools: vec!["primary".into(), "fallback".into()],
            throttle_registry: throttle_registry_for("primary", epoch_far_future()),
            rate_limit_fallback_enabled: false, // disabled
            now: "2026-03-18T12:00:00Z".into(),
            ..TaskSelectorConfig::default()
        };
        let selected = select_next_ready_task(&registry, &cfg).expect("primary selected (fallback disabled)");
        assert_eq!(selected.tool, "primary");
        assert!(!selected.is_fallback);
    }

    #[test]
    fn review_phase_task_does_not_fall_back() {
        let registry = json!({
          "tasks": [
            {
              "id": "T1",
              "title": "t1",
              "state": "todo",
              "priority": "1",
              "dependencies": [],
              "exclusive_resources": [],
              "task_runtime": { "current_phase": "review" }
            }
          ],
          "resource_locks": {}
        });
        let cfg = TaskSelectorConfig {
            default_tool: "fallback".into(),
            default_base_branch: "main".into(),
            max_parallel: 3,
            tool_priority: vec!["primary".into(), "fallback".into()],
            enabled_tools: vec!["primary".into(), "fallback".into()],
            throttle_registry: throttle_registry_for("primary", epoch_far_future()),
            rate_limit_fallback_enabled: true,
            now: "2026-03-18T12:00:00Z".into(),
            ..TaskSelectorConfig::default()
        };
        let selected = select_next_ready_task(&registry, &cfg).expect("primary selected (review phase)");
        assert_eq!(selected.tool, "primary");
        assert!(!selected.is_fallback, "review phase must not fall back");
    }

    #[test]
    fn fix_phase_task_does_not_fall_back() {
        let registry = json!({
          "tasks": [
            {
              "id": "T1",
              "title": "t1",
              "state": "todo",
              "priority": "1",
              "dependencies": [],
              "exclusive_resources": [],
              "task_runtime": { "current_phase": "fix" }
            }
          ],
          "resource_locks": {}
        });
        let cfg = TaskSelectorConfig {
            default_tool: "fallback".into(),
            default_base_branch: "main".into(),
            max_parallel: 3,
            tool_priority: vec!["primary".into(), "fallback".into()],
            enabled_tools: vec!["primary".into(), "fallback".into()],
            throttle_registry: throttle_registry_for("primary", epoch_far_future()),
            rate_limit_fallback_enabled: true,
            now: "2026-03-18T12:00:00Z".into(),
            ..TaskSelectorConfig::default()
        };
        let selected = select_next_ready_task(&registry, &cfg).expect("primary selected (fix phase)");
        assert_eq!(selected.tool, "primary");
        assert!(!selected.is_fallback, "fix phase must not fall back");
    }

    #[test]
    fn unthrottled_task_has_is_fallback_false() {
        let registry = json!({
          "tasks": [
            {"id":"T1","title":"t1","state":"todo","priority":"1","dependencies":[],"exclusive_resources":[]}
          ],
          "resource_locks": {}
        });
        let cfg = TaskSelectorConfig {
            default_tool: "primary".into(),
            default_base_branch: "main".into(),
            max_parallel: 3,
            tool_priority: vec!["primary".into()],
            enabled_tools: vec!["primary".into()],
            throttle_registry: ToolThrottleRegistry::default(),
            rate_limit_fallback_enabled: true,
            now: "2026-03-18T12:00:00Z".into(),
            ..TaskSelectorConfig::default()
        };
        let selected = select_next_ready_task(&registry, &cfg).expect("primary selected");
        assert_eq!(selected.tool, "primary");
        assert!(!selected.is_fallback);
    }
}

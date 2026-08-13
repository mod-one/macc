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
    /// Task IDs known to be merged from outside the current registry — typically
    /// prior PRD lots whose commits live on the reference branch. Used to
    /// satisfy cross-lot dependency edges that would otherwise look "unmet"
    /// because the dependency target isn't part of this registry's `tasks`.
    pub external_merged_ids: HashSet<String>,
    /// How many times a task parked by the same-worktree retry path may be
    /// re-dispatched into its existing worktree. Beyond this the task is left
    /// for `apply_stale_heartbeat_policy` / operator action rather than being
    /// retried forever. `0` disables same-worktree resume entirely.
    pub max_same_worktree_retries: usize,
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
    /// Set when the task is being re-dispatched into the worktree it already
    /// holds, after a tool reported `error_with_changes` on top of committed
    /// work. The dispatcher must resume in this worktree rather than acquiring
    /// (and resetting) a pool slot, or the commits would be stranded.
    pub resume_worktree: Option<ResumeWorktree>,
}

/// The worktree a parked task must resume in. See
/// [`crate::coordinator::model::Task::is_awaiting_same_worktree_retry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeWorktree {
    pub path: String,
    pub branch: String,
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
        if !dependencies_ready(task, &merged_ids, &config.external_merged_ids) {
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
        // A `todo` task normally must not carry a worktree -- that would mean it
        // is still assigned somewhere. The one exception is a task parked for a
        // same-worktree retry, which keeps its worktree on purpose so the retry
        // can resume on top of commits the tool already made.
        let resume_worktree = if task.has_worktree_attached() {
            match resume_worktree_for(task, config.max_same_worktree_retries) {
                Some(resume) => Some(resume),
                None => continue,
            }
        } else {
            None
        };
        if task.id.is_empty() {
            continue;
        }
        if !dependencies_ready(task, &merged_ids, &config.external_merged_ids) {
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
                resume_worktree,
            },
        ));
    }

    candidates.sort_by(|a, b| (&a.0, &a.1, &a.2).cmp(&(&b.0, &b.1, &b.2)));
    candidates
        .into_iter()
        .next()
        .map(|(_, _, _, selected)| selected)
}

/// Return the worktree a parked task should resume in, or `None` when the task
/// is not a same-worktree retry candidate or has exhausted its attempts.
///
/// The attempt counter is advanced when the task is parked (see
/// `engine::transitions`), so a task that keeps failing stops being selected
/// instead of cycling forever against the same broken state.
fn resume_worktree_for(task: &Task, max_retries: usize) -> Option<ResumeWorktree> {
    if max_retries == 0 || !task.is_awaiting_same_worktree_retry() {
        return None;
    }
    if task.task_runtime.retries_count() > max_retries {
        return None;
    }
    Some(ResumeWorktree {
        path: task.worktree_path()?.to_string(),
        branch: task.branch()?.to_string(),
    })
}

fn dependencies_ready(
    task: &Task,
    merged_ids: &HashSet<String>,
    external_merged_ids: &HashSet<String>,
) -> bool {
    task.dependency_ids().iter().all(|dependency| {
        merged_ids.contains(dependency) || external_merged_ids.contains(dependency)
    })
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
    // to the next available one.
    //
    // For review/fix phases the task is mid-flight and normally must not
    // switch tools (idempotency guard).  However, when the task's *own*
    // tool is throttled (e.g. E602 quota exhaustion), the worktree has
    // already been rolled back to pre-phase state by the caller, so a
    // tool switch is safe and necessary to make progress.
    let in_mid_flight_phase = matches!(
        task.task_runtime.current_phase.as_deref(),
        Some("review") | Some("fix")
    );
    let own_tool_throttled = task
        .tool
        .as_deref()
        .map(|t| is_tool_throttled(&config.throttle_registry, t, &config.now))
        .unwrap_or(false);
    let apply_throttle_filter = config.rate_limit_fallback_enabled
        && !config.throttle_registry.is_empty()
        && (!in_mid_flight_phase || own_tool_throttled);

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
    fn external_merged_ids_satisfy_cross_lot_dependency() {
        // The dep "PRIOR-LOT-001" is not in the current registry (it was
        // delivered by an earlier PRD lot and lives only as a commit on the
        // reference branch). With external_merged_ids populated from the
        // commit-trailer scan, the task must still dispatch.
        let registry = json!({
          "tasks": [
            {"id":"CURRENT","title":"current","state":"todo","priority":"1","dependencies":["PRIOR-LOT-001"],"exclusive_resources":[]}
          ],
          "resource_locks": {}
        });
        let cfg = TaskSelectorConfig {
            default_tool: "codex".into(),
            default_base_branch: "master".into(),
            max_parallel: 3,
            external_merged_ids: ["PRIOR-LOT-001".to_string()].into_iter().collect(),
            ..TaskSelectorConfig::default()
        };
        let selected = select_next_ready_task(&registry, &cfg).expect("selected task");
        assert_eq!(selected.id, "CURRENT");
    }

    #[test]
    fn external_merged_ids_empty_still_blocks_unmet_dependency() {
        // Without external_merged_ids, a dep absent from the registry must
        // continue to block dispatch — the new feature is additive, not a
        // loosening of dependency semantics.
        let registry = json!({
          "tasks": [
            {"id":"CURRENT","title":"current","state":"todo","priority":"1","dependencies":["PRIOR-LOT-001"],"exclusive_resources":[]}
          ],
          "resource_locks": {}
        });
        let cfg = TaskSelectorConfig {
            default_tool: "codex".into(),
            default_base_branch: "master".into(),
            max_parallel: 3,
            ..TaskSelectorConfig::default()
        };
        assert!(select_next_ready_task(&registry, &cfg).is_none());
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
        let selected =
            select_next_ready_task(&registry, &cfg).expect("should select eligible task");
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
        let selected =
            select_next_ready_task(&registry, &cfg).expect("should select undelayed task");
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
        let selected =
            select_next_ready_task(&registry, &cfg).expect("should select non-delayed task");
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
        let selected =
            select_next_ready_task(&registry, &cfg).expect("filter disabled → task selected");
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
        let selected =
            select_next_ready_task(&registry, &cfg).expect("primary selected after expiry");
        assert_eq!(selected.tool, "primary");
        assert!(
            !selected.is_fallback,
            "is_fallback must be false when throttle expired"
        );
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
        let selected =
            select_next_ready_task(&registry, &cfg).expect("primary selected (fallback disabled)");
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
        let selected =
            select_next_ready_task(&registry, &cfg).expect("primary selected (review phase)");
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
        let selected =
            select_next_ready_task(&registry, &cfg).expect("primary selected (fix phase)");
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

    // ── Same-worktree retry ────────────────────────────────────────────────
    //
    // A tool that commits work and then reports `error_with_changes` is
    // requeued to `todo` with its worktree deliberately kept. Before this was
    // handled, the selector's blanket "skip todo tasks with a worktree" rule
    // made those tasks permanently unschedulable: not active, not blocked, so
    // no recovery sweep reclaimed them either, and the run died with a
    // "made no progress" error while committed work sat stranded on a branch.

    fn parked_registry(retries: i64) -> serde_json::Value {
        json!({
          "tasks": [
            {
              "id":"T-PARKED","title":"parked","state":"todo","priority":"1",
              "dependencies":[],"exclusive_resources":[],
              "worktree":{
                "worktree_path":"/repo/.macc/worktree/worker-01",
                "branch":"ai/codex/worker-01-2026",
                "base_branch":"main",
                "last_commit":"abc123"
              },
              "task_runtime":{"status":"failed","retries":retries}
            }
          ],
          "resource_locks": {}
        })
    }

    fn parked_cfg(max_same_worktree_retries: usize) -> TaskSelectorConfig {
        TaskSelectorConfig {
            default_tool: "codex".into(),
            default_base_branch: "main".into(),
            max_parallel: 3,
            tool_priority: vec!["codex".into()],
            enabled_tools: vec!["codex".into()],
            max_same_worktree_retries,
            ..TaskSelectorConfig::default()
        }
    }

    #[test]
    fn parked_task_is_selected_and_resumes_its_own_worktree() {
        let selected = select_next_ready_task(&parked_registry(1), &parked_cfg(2))
            .expect("a task parked for same-worktree retry must be dispatchable");
        assert_eq!(selected.id, "T-PARKED");
        let resume = selected
            .resume_worktree
            .expect("dispatch must be told to resume the attached worktree");
        assert_eq!(resume.path, "/repo/.macc/worktree/worker-01");
        assert_eq!(resume.branch, "ai/codex/worker-01-2026");
    }

    #[test]
    fn parked_task_stops_being_selected_once_attempts_are_exhausted() {
        // retries (2) exceeds the budget (1) -> no longer eligible, so a task
        // that keeps failing cannot spin the dispatcher forever.
        assert!(select_next_ready_task(&parked_registry(2), &parked_cfg(1)).is_none());
    }

    #[test]
    fn same_worktree_resume_can_be_disabled() {
        assert!(select_next_ready_task(&parked_registry(0), &parked_cfg(0)).is_none());
    }

    #[test]
    fn a_normally_assigned_task_is_still_skipped() {
        // Guard against loosening the original rule: a `todo` task holding a
        // worktree while its runtime is *not* failed is still mid-assignment
        // and must not be picked up.
        let registry = json!({
          "tasks": [
            {
              "id":"T-ASSIGNED","title":"assigned","state":"todo","priority":"1",
              "dependencies":[],"exclusive_resources":[],
              "worktree":{
                "worktree_path":"/repo/.macc/worktree/worker-01",
                "branch":"ai/codex/worker-01-2026"
              },
              "task_runtime":{"status":"running"}
            }
          ],
          "resource_locks": {}
        });
        assert!(select_next_ready_task(&registry, &parked_cfg(2)).is_none());
    }

    #[test]
    fn parked_task_without_a_branch_is_not_resumable() {
        // No branch means there are no commits to preserve; such a task must
        // not claim a resume slot.
        let registry = json!({
          "tasks": [
            {
              "id":"T-NOBRANCH","title":"no branch","state":"todo","priority":"1",
              "dependencies":[],"exclusive_resources":[],
              "worktree":{"worktree_path":"/repo/.macc/worktree/worker-01"},
              "task_runtime":{"status":"failed"}
            }
          ],
          "resource_locks": {}
        });
        assert!(select_next_ready_task(&registry, &parked_cfg(2)).is_none());
    }

    #[test]
    fn a_clean_todo_task_carries_no_resume_worktree() {
        let registry = json!({
          "tasks": [
            {"id":"T-FRESH","title":"fresh","state":"todo","priority":"1","dependencies":[],"exclusive_resources":[]}
          ],
          "resource_locks": {}
        });
        let selected = select_next_ready_task(&registry, &parked_cfg(2)).expect("selected");
        assert_eq!(selected.id, "T-FRESH");
        assert!(selected.resume_worktree.is_none());
    }
}

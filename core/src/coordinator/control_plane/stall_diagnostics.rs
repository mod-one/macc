//! Root-cause summaries without changing dispatch or cooldown eligibility.
use super::*;
/// Explain why no `todo` task could be dispatched, for the "no progress" abort.
///
/// Builds the *same* selector config dispatch uses, so the reasons reported are
/// the reasons that actually applied. Where a task still holds a branch, the
/// count of unmerged commits on it is appended -- that is the number an operator
/// needs to decide between recovering the work and discarding it.
pub fn diagnose_stall_native(
    repo_root: &Path,
    canonical: &crate::config::CanonicalConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    state: &CoordinatorRunState,
) -> crate::coordinator::engine::StallDiagnosis {
    let cfg = CoordinatorConfigResolved::resolve(coordinator);
    let Ok(registry_value) =
        crate::coordinator::state::coordinator_state_registry_load(repo_root, &BTreeMap::new())
    else {
        return crate::coordinator::engine::StallDiagnosis::default();
    };
    let Ok(registry) = crate::coordinator::model::TaskRegistry::from_value(&registry_value) else {
        return crate::coordinator::engine::StallDiagnosis::default();
    };
    let selector_cfg = build_task_selector_config(
        repo_root,
        &registry_value,
        canonical,
        env_cfg,
        &cfg,
        coordinator,
        state,
    );
    let base_branch = selector_cfg.default_base_branch.clone();

    let todo_count = registry
        .tasks
        .iter()
        .filter(|task| task.workflow_state() == Some(crate::coordinator::WorkflowState::Todo))
        .count();
    let diagnosed =
        crate::coordinator::task_selector::diagnose_unschedulable_tasks(&registry, &selector_cfg);
    let terminal_dependents = diagnosed
        .iter()
        .filter(|task| {
            task.depends_on_blocked
                && registry
                    .find_task(&task.id)
                    .is_some_and(|t| t.state == "todo")
        })
        .count();
    let has_blocked_root = diagnosed.iter().any(|task| task.blocked_root);
    let affected: Vec<_> = diagnosed
        .iter()
        .filter(|d| d.depends_on_blocked)
        .map(|d| d.id.clone())
        .collect();
    let mut lines: Vec<String> = diagnosed
        .into_iter()
        .filter(|d| !has_blocked_root || !d.depends_on_blocked)
        .map(|stuck| {
            let mut line = format!("{} ({})", stuck.id, stuck.reason);
            if let Some(branch) = stuck.branch {
                let commits = crate::git::commits_between(repo_root, &base_branch, &branch)
                    .map(|c| c.len())
                    .unwrap_or(0);
                line.push_str(&format!(
                    "; branch {} holds {} unmerged commit(s)",
                    branch, commits
                ));
            }
            line
        })
        .collect();
    if has_blocked_root && !affected.is_empty() {
        lines.push(format!(
            "Affected dependent task(s): {}",
            affected.join(", ")
        ));
    }
    crate::coordinator::engine::StallDiagnosis {
        lines,
        terminal_blocked: has_blocked_root
            && (todo_count == 0 || terminal_dependents == todo_count),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grouped_dependency_fallout_cannot_stop_independent_work_waiting_for_quota() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = crate::config::CanonicalConfig::default();
        config.tools.enabled = vec!["codex".into()];
        let mut state = CoordinatorRunState::new();
        state.throttle_registry.insert(
            "codex".into(),
            crate::coordinator::rate_limit::ToolThrottleState {
                tool_id: "codex".into(),
                throttled_until: chrono::Utc::now().timestamp() as u64 + 3600,
                ..Default::default()
            },
        );
        let mut value = serde_json::json!({"tasks":[
            {"id":"ROOT","state":"blocked","task_runtime":{"last_error_code":"E103","last_error_message":"Missing ACK"}},
            {"id":"BLOCKED-CHILD","state":"blocked","dependencies":["ROOT"],"task_runtime":{"last_error_code":"E905"}},
            {"id":"TODO-CHILD","state":"todo","dependencies":["ROOT"],"tool":"codex"},
            {"id":"INDEPENDENT","state":"todo","tool":"codex"}
        ]});
        let inspect = |value: &serde_json::Value| {
            crate::coordinator::state::coordinator_state_registry_save(
                tmp.path(),
                &BTreeMap::new(),
                value,
            )
            .unwrap();
            diagnose_stall_native(tmp.path(), &config, None, &Default::default(), &state)
        };
        let summary = inspect(&value);
        assert!(!summary.terminal_blocked);
        assert_eq!(
            summary
                .lines
                .iter()
                .filter(|l| l.starts_with("ROOT ("))
                .count(),
            1
        );
        assert!(summary
            .lines
            .iter()
            .any(|l| l.starts_with("Affected dependent task(s):")
                && l.contains("BLOCKED-CHILD")
                && l.contains("TODO-CHILD")));
        value["tasks"].as_array_mut().unwrap().pop();
        assert!(inspect(&value).terminal_blocked);
    }
}

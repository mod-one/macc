//! Root-cause summaries without changing dispatch or cooldown eligibility.
use super::*;
use crate::coordinator::engine::StallDisposition;
use crate::coordinator::task_selector::DispatchWait;
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
    let explained_todos = diagnosed
        .iter()
        .filter(|task| {
            (task.cause != DispatchWait::Other)
                && registry
                    .find_task(&task.id)
                    .is_some_and(|t| t.state == "todo")
        })
        .count();
    let has_blocked_root = diagnosed
        .iter()
        .any(|task| task.cause == DispatchWait::BlockedRoot);
    let disposition = if explained_todos != todo_count {
        StallDisposition::Unexplained
    } else if crate::coordinator::decisions::human_wait_count(&registry) > 0 {
        StallDisposition::WaitingForApproval
    } else if has_blocked_root {
        StallDisposition::Blocked
    } else {
        StallDisposition::Unexplained
    };
    let affected: Vec<_> = diagnosed
        .iter()
        .filter(|d| d.cause == DispatchWait::BlockedDependency)
        .map(|d| d.id.clone())
        .collect();
    let mut lines: Vec<String> = diagnosed
        .into_iter()
        .filter(|d| !has_blocked_root || d.cause != DispatchWait::BlockedDependency)
        .map(|stuck| {
            let mut line = format!("{} ({})", stuck.id, stuck.reason);
            // Branch recovery details are needed for a stopping run. Avoid Git
            // subprocesses on the periodic recheck of an expected approval wait.
            if let Some(branch) = stuck
                .branch
                .filter(|_| disposition != StallDisposition::WaitingForApproval)
            {
                match crate::git::commits_between(repo_root, &base_branch, &branch) {
                    Ok(commits) => line.push_str(&format!(
                        "; branch {branch} holds {} unmerged commit(s)",
                        commits.len()
                    )),
                    Err(_) => line.push_str(&format!(
                        "; could not verify branch {branch} against {base_branch}"
                    )),
                }
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
    crate::coordinator::engine::StallDiagnosis { lines, disposition }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_external_intervention_groups_transitive_consumers_but_keeps_unrelated_errors() {
        let root = tempfile::tempdir().unwrap();
        let mut tasks = serde_json::json!([
            {"id":"MOBILE","state":"blocked"},
            {"id":"REVIEW","state":"blocked","dependencies":["MOBILE"]},
            {"id":"DOC","state":"blocked","dependencies":["REVIEW"]},
            {"id":"UNRELATED","state":"blocked","dependencies":["MOBILE"],"task_runtime":{"last_error_code":"E103","last_error_message":"Missing native ACK"}}
        ]);
        for task in tasks.as_array_mut().unwrap().iter_mut().take(3) {
            task["task_runtime"] = serde_json::json!({"last_error_code":"E903","last_error_origin":"decisions","decision_wait":"device","last_error_message":"Connect Android device; No USB access"});
        }
        // The older registry format has no owner field: graph fallback must work.
        let registry = serde_json::json!({"tasks":tasks});
        crate::coordinator::state::coordinator_state_registry_save(
            root.path(),
            &BTreeMap::new(),
            &registry,
        )
        .unwrap();
        let summary = diagnose_stall_native(
            root.path(),
            &Default::default(),
            None,
            &Default::default(),
            &CoordinatorRunState::new(),
        );
        assert!(summary
            .lines
            .iter()
            .any(|l| l.starts_with("MOBILE (") && l.contains("No USB access")));
        assert!(summary
            .lines
            .iter()
            .any(|l| l.starts_with("UNRELATED (") && l.contains("E103")));
        assert!(!summary
            .lines
            .iter()
            .any(|l| l.starts_with("REVIEW (") || l.starts_with("DOC (")));
        assert!(summary
            .lines
            .iter()
            .any(|l| l.contains("Affected dependent task(s)")
                && l.contains("DOC")
                && l.contains("REVIEW")));
    }
    #[test]
    fn mixed_external_and_approval_waits_survive_idle_cycles_and_name_the_gate() {
        use crate::coordinator::decisions::human_wait_count;
        use crate::coordinator::engine::{
            ControlPlaneDecision, ControlPlaneLoopConfig, CoordinatorCounts,
            CoordinatorRunController,
        };
        use crate::coordinator::model::TaskRegistry;

        let root = tempfile::tempdir().unwrap();
        let value = serde_json::json!({"tasks": [
            {"id":"DEVICE","state":"blocked","task_runtime":{"last_error_code":"E903","last_error_origin":"decisions","decision_wait":"device","decision_wait_status":"blocked_external","last_error_message":"Connect the test device"}},
            {"id":"REVIEW","state":"expired","gate":{"kind":"human_approval"}},
            {"id":"BUILD","state":"todo","dependencies":["REVIEW"]},
            {"id":"SHIP","state":"todo","dependencies":["BUILD"]}
        ]});
        let mut registry = TaskRegistry::from_value(&value).unwrap();
        let diagnose = |registry: &TaskRegistry| {
            crate::coordinator::state::coordinator_state_registry_save(
                root.path(),
                &BTreeMap::new(),
                &registry.to_value().unwrap(),
            )
            .unwrap();
            diagnose_stall_native(
                root.path(),
                &Default::default(),
                None,
                &Default::default(),
                &CoordinatorRunState::new(),
            )
        };
        let selector = crate::coordinator::task_selector::TaskSelectorConfig {
            default_tool: "codex".into(),
            enabled_tools: vec!["codex".into()],
            ..Default::default()
        };
        let next = |registry: &TaskRegistry| {
            crate::coordinator::task_selector::select_next_ready_task_typed(registry, &selector)
                .map(|task| task.id)
        };
        assert_eq!(next(&registry), None);
        let summary = diagnose(&registry);
        assert_eq!(
            summary.disposition,
            StallDisposition::WaitingForApproval,
            "each todo has a recorded prerequisite"
        );
        assert!(summary
            .lines
            .iter()
            .any(|line| line.contains("SHIP -> BUILD -> REVIEW")));
        assert!(summary
            .lines
            .iter()
            .any(|line| line.contains("Connect the test device")));
        let mut controller = CoordinatorRunController::new(ControlPlaneLoopConfig {
            timeout: None,
            max_no_progress_cycles: 5,
        });
        let counts = CoordinatorCounts {
            total: 4,
            todo: 2,
            active: 0,
            blocked: 1,
            merged: 0,
            waiting_approval: human_wait_count(&registry),
        };
        assert_eq!(counts.waiting_approval, 1);
        for _ in 0..8 {
            assert_eq!(
                controller
                    .on_cycle_counts_with(counts, None, || summary.clone())
                    .unwrap(),
                ControlPlaneDecision::Continue
            );
        }
        // An unrelated dispatch fault must still trip the watchdog.
        let mut with_unrelated = registry.clone();
        with_unrelated.tasks.push(
            serde_json::from_value(
                serde_json::json!({"id":"UNRELATED","state":"todo","dependencies":["UNKNOWN"]}),
            )
            .unwrap(),
        );
        let independent = diagnose(&with_unrelated);
        assert_eq!(independent.disposition, StallDisposition::Unexplained);
        let mut fault = CoordinatorRunController::new(ControlPlaneLoopConfig {
            timeout: None,
            max_no_progress_cycles: 1,
        });
        let counts = CoordinatorCounts {
            total: 5,
            todo: 3,
            ..counts
        };
        fault
            .on_cycle_counts_with(counts, None, || independent.clone())
            .unwrap();
        assert!(fault
            .on_cycle_counts_with(counts, None, || independent)
            .unwrap_err()
            .to_string()
            .contains("UNKNOWN"));
        registry.find_task_mut("REVIEW").unwrap().state = "approved".into();
        assert_eq!(next(&registry).as_deref(), Some("BUILD"));
        assert_eq!(human_wait_count(&registry), 0);
        assert_eq!(
            diagnose(&registry).disposition,
            StallDisposition::Unexplained,
            "the independent build is now runnable"
        );
    }

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
        assert_eq!(summary.disposition, StallDisposition::Unexplained);
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
        assert_eq!(inspect(&value).disposition, StallDisposition::Blocked);
    }
}

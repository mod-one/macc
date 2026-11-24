use super::*;
use crate::{
    catalog::Agent,
    config::CanonicalConfig,
    coordinator::{model::Task, review_budget::ReviewExecution, state},
    coordinator_storage::CoordinatorSnapshot,
    doctor::ToolCheck,
    engine::Engine,
    plan::{ActionPlan, PlannedOp},
    resolve::{CliOverrides, MaterializedFetchUnit},
    tool::{ToolDescriptor, ToolDiagnostic, ToolRegistry},
    ApplyReport, MaccEngine,
};
use serde_json::json;
use std::{cell::Cell, path::Path};

struct ConcurrentWriter {
    inner: MaccEngine,
    reads: Cell<usize>,
}

impl Engine for ConcurrentWriter {
    fn list_tools(&self, p: &ProjectPaths) -> (Vec<ToolDescriptor>, Vec<ToolDiagnostic>) {
        self.inner.list_tools(p)
    }
    fn doctor(&self, p: &ProjectPaths) -> Vec<ToolCheck> {
        self.inner.doctor(p)
    }
    fn plan(
        &self,
        p: &ProjectPaths,
        c: &CanonicalConfig,
        u: &[MaterializedFetchUnit],
        o: &CliOverrides,
    ) -> Result<ActionPlan> {
        self.inner.plan(p, c, u, o)
    }
    fn plan_operations(&self, p: &ProjectPaths, a: &ActionPlan) -> Vec<PlannedOp> {
        self.inner.plan_operations(p, a)
    }
    fn apply(&self, p: &ProjectPaths, a: &mut ActionPlan, u: bool) -> Result<ApplyReport> {
        self.inner.apply(p, a, u)
    }
    fn builtin_agents(&self) -> Vec<Agent> {
        self.inner.builtin_agents()
    }

    fn coordinator_state_snapshot(
        &self,
        root: &Path,
        args: &BTreeMap<String, String>,
    ) -> Result<CoordinatorSnapshot> {
        let snapshot = state::coordinator_state_snapshot(root, args)?;
        // Commit the other client's write after the read, before either retry write.
        let mut concurrent = snapshot.registry.to_value()?;
        let reads = self.reads.get();
        if reads == 0 {
            concurrent["tasks"][1]["state"] = json!("merged");
        } else {
            concurrent["tasks"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id":"C","state":"todo"}));
        }
        state::coordinator_state_registry_save(root, args, &concurrent)?;
        self.reads.set(reads + 1);
        Ok(snapshot)
    }

    fn coordinator_run_review_phase_for_task_native(
        &self,
        _: &Path,
        _: &Task,
        _: Option<&str>,
        _: Option<&dyn CoordinatorLog>,
    ) -> Result<ReviewExecution> {
        Ok(ReviewExecution {
            attempts: 1,
            outcome: Ok(coordinator_engine::ReviewVerdict::Ok),
        })
    }
    fn coordinator_run_phase_for_task_native(
        &self,
        _: &Path,
        _: &Task,
        phase: &str,
        _: Option<&str>,
        _: usize,
        _: Option<&dyn CoordinatorLog>,
    ) -> Result<std::result::Result<String, String>> {
        assert_eq!(phase, "fix");
        Ok(Ok(String::new()))
    }
}

#[test]
fn retry_preserves_other_clients_progress_and_new_tasks() {
    for mode in ["sqlite", "json", "dual-write"] {
        for (phase, expected) in [("review", "pr_open"), ("fix", "in_progress")] {
            let tmp = tempfile::tempdir().unwrap();
            let paths = ProjectPaths::from_root(tmp.path());
            let args = BTreeMap::from([("storage-mode".into(), mode.into())]);
            if phase == "review" {
                for args in [
                    vec!["init", "-qb", "main"],
                    vec!["config", "user.name", "Fixture"],
                    vec!["config", "user.email", "test@example.invalid"],
                    vec!["commit", "--allow-empty", "-qm", "base"],
                    vec!["worktree", "add", "-qb", "candidate", "worker"],
                    vec![
                        "-C",
                        "worker",
                        "commit",
                        "--allow-empty",
                        "-qm",
                        "candidate",
                    ],
                ] {
                    assert!(
                        crate::git::run_git_output_mapped(tmp.path(), &args, "review fixture")
                            .unwrap()
                            .status
                            .success()
                    );
                }
            }
            state::coordinator_state_registry_save(
                tmp.path(),
                &args,
                &json!({"tasks":[
                    {"id":"A","state":"blocked","worktree":{"worktree_path":"worker","branch":"candidate","base_branch":"main"},"task_runtime":{"current_phase":phase}},
                    {"id":"B","state":"todo"}
                ]}),
            )
            .unwrap();
            let engine = ConcurrentWriter {
                inner: MaccEngine::new(ToolRegistry::new()),
                reads: Cell::new(0),
            };
            coordinator_retry_phase(
                &engine,
                &paths,
                &Default::default(),
                None,
                &CoordinatorEnvConfig {
                    storage_mode: Some(mode.into()),
                    ..Default::default()
                },
                &["--retry-task", "A", "--retry-phase", phase].map(String::from),
                None,
            )
            .unwrap();
            let registry = state::coordinator_state_snapshot(tmp.path(), &args)
                .unwrap()
                .registry;
            assert_eq!(registry.find_task("A").unwrap().state, expected);
            assert_eq!(registry.find_task("B").unwrap().state, "merged");
            assert_eq!(registry.find_task("C").unwrap().state, "todo");
        }
    }
}

#[test]
fn unlock_preserves_other_locks_and_never_restores_the_last_lock_from_metadata() {
    for mode in ["sqlite", "json", "dual-write"] {
        let tmp = tempfile::tempdir().unwrap();
        let args = BTreeMap::from([("storage-mode".into(), mode.into())]);
        state::coordinator_state_registry_save(
            tmp.path(),
            &args,
            &json!({"tasks":[{"id":"B","state":"merged"}],
            "resource_locks":{"first":{"task_id":"A"}, "second":{"task_id":"B"}}}),
        )
        .unwrap();
        let unlock = |key, all| {
            state::coordinator_state_unlock_resource(tmp.path(), &args, key, all).unwrap()
        };
        assert_eq!(unlock(Some("first"), false), 1);
        assert_eq!(unlock(Some("first"), false), 0);
        assert_eq!(
            state::coordinator_state_snapshot(tmp.path(), &args)
                .unwrap()
                .registry
                .resource_locks["second"]
                .task_id,
            "B"
        );
        assert_eq!(unlock(None, true), 1);
        let registry = state::coordinator_state_snapshot(tmp.path(), &args)
            .unwrap()
            .registry;
        assert!(registry.resource_locks.is_empty());
        assert_eq!(registry.find_task("B").unwrap().state, "merged");
    }
}

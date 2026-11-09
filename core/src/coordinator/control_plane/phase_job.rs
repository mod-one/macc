//! Long phase processes never own the control-plane loop or its registry snapshot.
use super::{
    base::CoordinatorLog,
    phase_runner::{NativePhaseExecutor, PreparedPhaseContext},
};
use crate::coordinator::{
    engine::{PhaseTransition, ReviewVerdict},
    model::Task,
    runtime,
};
use crate::{MaccError, Result};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

pub(crate) enum PhaseOutcome {
    Review(std::result::Result<ReviewVerdict, String>),
    Other(std::result::Result<String, String>),
}

pub(crate) struct PhaseJob {
    pub task: Task,
    pub mode: &'static str,
    pub transition: PhaseTransition,
    pub reviewed_commit: Option<String>,
    pub worker: tokio::task::JoinHandle<Result<PhaseOutcome>>,
    cancelled: Arc<AtomicBool>,
    result: Option<Result<PhaseOutcome>>,
    logs: tokio::sync::mpsc::UnboundedReceiver<String>,
}

struct WorkerLog(tokio::sync::mpsc::UnboundedSender<String>);
impl CoordinatorLog for WorkerLog {
    fn note(&self, line: String) -> Result<()> {
        let _ = self.0.send(line);
        Ok(())
    }
}

impl PhaseJob {
    pub fn start(
        repo: PathBuf,
        task: Task,
        mode: &'static str,
        transition: PhaseTransition,
        tool: Option<String>,
        attempts: usize,
        wake: Arc<tokio::sync::Notify>,
    ) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = cancelled.clone();
        let (tx, logs) = tokio::sync::mpsc::unbounded_channel();
        let snapshot = task.clone();
        let reviewed_commit = if mode == "review" {
            crate::git::head_commit(std::path::Path::new(
                task.worktree_path().unwrap_or_default(),
            ))
            .ok()
        } else {
            None
        };
        // Context preparation updates the registry. Do it on the control
        // plane, after its snapshot save and before the worker can execute.
        let attempt = uuid::Uuid::new_v4().to_string();
        let prepared = crate::coordinator::decisions::prepare_context(
            &repo,
            &task.id,
            mode,
            std::path::Path::new(task.worktree_path().unwrap_or_default()),
            &attempt,
        );
        let worker = tokio::task::spawn_blocking(move || {
            // Also wake on panic: the loop must collect and report the JoinError.
            struct Wake(Arc<tokio::sync::Notify>);
            impl Drop for Wake {
                fn drop(&mut self) {
                    self.0.notify_one();
                }
            }
            let _wake = Wake(wake);
            let prepared_context = match prepared {
                Ok(context) => PreparedPhaseContext { attempt, context },
                Err(error)
                    if error
                        .to_string()
                        .contains(crate::coordinator::decisions::CONTEXT_BUDGET_CODE)
                        || error.to_string().contains("DM_PENDING:") =>
                {
                    let reason = format!("E903: {error}");
                    return Ok(if mode == "review" {
                        PhaseOutcome::Review(Err(reason))
                    } else {
                        PhaseOutcome::Other(Err(reason))
                    });
                }
                Err(error) => return Err(error),
            };
            let logger = WorkerLog(tx);
            let executor = NativePhaseExecutor {
                repo_root: &repo,
                logger: Some(&logger),
                cancelled: Some(&cancel),
                prepared_context: Some(&prepared_context),
            };
            if mode == "review" {
                runtime::run_review_phase(&executor, &snapshot, tool.as_deref(), attempts)
                    .map(PhaseOutcome::Review)
            } else {
                runtime::run_phase(&executor, &snapshot, mode, tool.as_deref(), attempts)
                    .map(PhaseOutcome::Other)
            }
        });
        Self {
            task,
            mode,
            transition,
            reviewed_commit,
            worker,
            cancelled,
            result: None,
            logs,
        }
    }

    pub fn matches_task(&self, task: &Task) -> bool {
        self.task.state == task.state
            && self.task.worktree_path() == task.worktree_path()
            && self.task.branch() == task.branch()
            && self.task.base_branch("master") == task.base_branch("master")
    }

    pub fn drain_logs(&mut self, logger: Option<&dyn CoordinatorLog>) {
        while let Ok(line) = self.logs.try_recv() {
            if let Some(log) = logger {
                let _ = log.note(line);
            }
        }
    }

    pub fn is_finished(&self) -> bool {
        self.result.is_some() || self.worker.is_finished()
    }

    pub async fn wait(&mut self) {
        if self.result.is_none() {
            self.result = Some(
                (&mut self.worker)
                    .await
                    .map_err(|e| MaccError::Validation(format!("Phase worker failed: {e}")))
                    .and_then(|result| result),
            );
        }
    }

    pub async fn finish(&mut self) -> Result<PhaseOutcome> {
        self.wait().await;
        self.result.take().expect("phase has finished")
    }

    pub async fn cancel_and_wait(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.wait().await;
    }
}

impl Drop for PhaseJob {
    fn drop(&mut self) {
        // spawn_blocking cannot be aborted once started. The process runner
        // observes this flag, kills its own process group and reaps its child.
        self.cancelled.store(true, Ordering::Release);
        self.worker.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator::{
        control_plane::{advance_tasks_native, monitor_active_jobs_native},
        runtime::{
            CoordinatorJob, CoordinatorJobEvent, CoordinatorMergeEvent, CoordinatorRunState,
        },
        state::{coordinator_state_registry_load, coordinator_state_registry_save},
        PerformerCompletionKind, WorkflowState,
    };
    use std::{
        collections::BTreeMap,
        time::{Duration, Instant},
    };

    fn task() -> Task {
        serde_json::from_value(serde_json::json!({"id":"REVIEW", "state":"testing"})).unwrap()
    }

    fn slow_job(task: Task) -> (PhaseJob, std::sync::mpsc::Sender<()>) {
        let (release, rx) = std::sync::mpsc::channel();
        let (_, logs) = tokio::sync::mpsc::unbounded_channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = cancelled.clone();
        let worker = tokio::task::spawn_blocking(move || {
            while !cancel.load(Ordering::Acquire) {
                match rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
            Ok(PhaseOutcome::Review(Ok(ReviewVerdict::Ok)))
        });
        (
            PhaseJob {
                task,
                mode: "review",
                transition: PhaseTransition {
                    mode: "review",
                    next_state: WorkflowState::Reviewing,
                    runtime_phase: "review",
                },
                reviewed_commit: None,
                worker,
                cancelled,
                logs,
                result: None,
            },
            release,
        )
    }

    fn state_of<'a>(registry: &'a serde_json::Value, id: &str) -> &'a str {
        registry["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == id)
            .unwrap()["state"]
            .as_str()
            .unwrap()
    }

    fn exit(kind: PerformerCompletionKind) -> CoordinatorJobEvent {
        CoordinatorJobEvent {
            task_id: "DEV".into(),
            success: matches!(
                kind,
                PerformerCompletionKind::SuccessWithoutChanges
                    | PerformerCompletionKind::SuccessWithChanges
                    | PerformerCompletionKind::AlreadySatisfied
            ),
            status_text: kind.as_str().into(),
            timed_out: false,
            completion_kind: Some(kind),
            completion_details_source: Some("sqlite".into()),
            normalizer_input: None,
            error_code: None,
            error_origin: None,
            error_message: None,
            result_explanation: None,
            gate_verdict: None,
            unmet_preconditions: if kind == PerformerCompletionKind::PreconditionUnmet {
                vec!["device unavailable".into()]
            } else {
                vec![]
            },
        }
    }

    #[tokio::test]
    async fn exits_wake_idle_loop_without_losing_success_or_error_events() {
        for kind in [
            PerformerCompletionKind::SuccessWithoutChanges,
            PerformerCompletionKind::PreconditionUnmet,
        ] {
            let mut state = CoordinatorRunState::new();
            let tx = state.event_tx.clone();
            let sender = tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                tx.send(exit(kind)).unwrap();
            });
            tokio::time::timeout(
                Duration::from_secs(1),
                state.wait_for_activity(Duration::from_secs(30)),
            )
            .await
            .expect("completion must interrupt the idle timer");
            sender.await.unwrap();
            assert_eq!(
                state.pending_job_event.take().unwrap().completion_kind,
                Some(kind)
            );
            assert!(
                state.event_rx.try_recv().is_err(),
                "event must be retained exactly once"
            );
        }
        let mut state = CoordinatorRunState::new();
        state
            .merge_event_tx
            .send(CoordinatorMergeEvent {
                task_id: "MERGE".into(),
                success: true,
                reason: "ok".into(),
            })
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(1),
            state.wait_for_activity(Duration::from_secs(30)),
        )
        .await
        .unwrap();
        assert_eq!(state.pending_merge_event.take().unwrap().task_id, "MERGE");
        state.activity_wake.notify_one(); // An exit before wait registration must not be lost.
        tokio::time::timeout(
            Duration::from_secs(1),
            state.wait_for_activity(Duration::from_secs(30)),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn long_review_does_not_delay_precondition_exit_or_overwrite_its_result() {
        let root = tempfile::tempdir().unwrap();
        let tasks = serde_json::json!({"tasks":[{"id":"REVIEW","state":"testing"},{"id":"DEV","state":"claimed","task_runtime":{"status":"running","current_phase":"dev"}}]});
        coordinator_state_registry_save(root.path(), &BTreeMap::new(), &tasks).unwrap();
        let mut state = CoordinatorRunState::new();
        let (job, release) = slow_job(task());
        state.phase_job = Some(job);
        state.active_jobs.insert(
            "DEV".into(),
            CoordinatorJob {
                tool: "codex".into(),
                base_branch: "main".into(),
                worktree_path: root.path().to_path_buf(),
                attempt: 1,
                started_at: Instant::now(),
                pid: None,
                failure_signaled_at: None,
            },
        );
        let env = Default::default();
        tokio::time::timeout(
            Duration::from_secs(1),
            advance_tasks_native(root.path(), &env, None, None, 1, &mut state, None),
        )
        .await
        .expect("advance must not wait for the review worker")
        .unwrap();
        assert!(!state.phase_job.as_ref().unwrap().is_finished());
        state
            .event_tx
            .send(exit(PerformerCompletionKind::PreconditionUnmet))
            .unwrap();
        state.wait_for_activity(Duration::from_secs(30)).await;
        monitor_active_jobs_native(root.path(), &env, None, &mut state, 1, 0, None)
            .await
            .unwrap();
        let registry = coordinator_state_registry_load(root.path(), &BTreeMap::new()).unwrap();
        assert_eq!(state_of(&registry, "DEV"), "blocked");
        assert!(!state.active_jobs.contains_key("DEV"));
        assert!(
            !state.phase_job.as_ref().unwrap().is_finished(),
            "DEV result must be applied before review completion"
        );
        release.send(()).unwrap();
        // The first wait can return for another event. Collect the actual phase deterministically.
        state.phase_job.as_mut().unwrap().wait().await;
        advance_tasks_native(root.path(), &env, None, None, 1, &mut state, None)
            .await
            .unwrap();
        let registry = coordinator_state_registry_load(root.path(), &BTreeMap::new()).unwrap();
        assert_eq!(
            state_of(&registry, "DEV"),
            "blocked",
            "phase completion must not overwrite another task outcome"
        );
        assert_eq!(state_of(&registry, "REVIEW"), "pr_open");
        assert!(state.phase_job.is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn real_process_exit_wakes_before_missing_ipc_reconciliation_finishes() {
        use std::os::unix::fs::PermissionsExt;
        for code in [0, 7] {
            let root = tempfile::tempdir().unwrap();
            let runner = root.path().join("exits.sh");
            std::fs::write(&runner, format!("#!/bin/sh\nexit {code}\n")).unwrap();
            std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o755)).unwrap();
            let mut state = CoordinatorRunState::new();
            runtime::spawn_performer_job(
                &runner,
                root.path(),
                "DEV",
                "main",
                root.path(),
                &state.event_tx,
                &mut state.join_set,
                0,
                Some("127.0.0.1:9"),
                "claim",
                1,
                &[],
                state.activity_wake.clone(),
            )
            .unwrap();
            tokio::time::timeout(
                Duration::from_secs(1),
                state.wait_for_activity(Duration::from_secs(30)),
            )
            .await
            .expect("OS exit must wake immediately, even with no terminal IPC record");
            assert!(state.pending_job_event.is_none());
            assert!(
                state.event_rx.try_recv().is_err(),
                "an OS exit alone must not fabricate completion evidence"
            );
            state.join_set.shutdown().await;
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_test_launch_returns_while_external_script_is_still_running() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(".macc/state")).unwrap();
        let runner = root.path().join("slow-test.sh");
        std::fs::write(
            &runner,
            "#!/bin/sh\nprintf started > .macc/started\nsleep 30\n",
        )
        .unwrap();
        std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(root.path().join(".macc/tool.json"), serde_json::json!({"id":"codex","performer":{"runner":runner,"session":{"enabled":false}}}).to_string()).unwrap();
        std::fs::write(
            root.path().join(".macc/state/coordinator.ipc.addr"),
            "127.0.0.1:9",
        )
        .unwrap();
        let tasks = serde_json::json!({"tasks":[{"id":"TEST","state":"in_progress","tool":"codex","worktree":{"worktree_path":root.path()},"task_runtime":{"status":"phase_done","current_phase":"dev"}}]});
        coordinator_state_registry_save(root.path(), &BTreeMap::new(), &tasks).unwrap();
        let mut state = CoordinatorRunState::new();
        let env = crate::coordinator::types::CoordinatorEnvConfig {
            testing_mode: Some("ai".into()),
            ..Default::default()
        };
        tokio::time::timeout(
            Duration::from_secs(2),
            advance_tasks_native(root.path(), &env, None, None, 1, &mut state, None),
        )
        .await
        .expect("native launch must not wait for the external test")
        .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !root.path().join(".macc/started").exists() {
                assert!(
                    !state.phase_job.as_ref().unwrap().is_finished(),
                    "runner must have started"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture script must start");
        assert!(!state.phase_job.as_ref().unwrap().is_finished());
        tokio::time::timeout(
            Duration::from_secs(1),
            state.phase_job.as_mut().unwrap().cancel_and_wait(),
        )
        .await
        .expect("stop must reap the external test");
    }

    #[tokio::test]
    async fn worker_completion_wakes_and_retains_result_and_cancellation_joins() {
        let mut state = CoordinatorRunState::new();
        let (job, release) = slow_job(task());
        state.phase_job = Some(job);
        release.send(()).unwrap();
        tokio::time::timeout(
            Duration::from_secs(1),
            state.wait_for_activity(Duration::from_secs(30)),
        )
        .await
        .unwrap();
        assert!(state.phase_job.as_ref().unwrap().is_finished());
        assert!(matches!(
            state.phase_job.as_mut().unwrap().finish().await.unwrap(),
            PhaseOutcome::Review(Ok(ReviewVerdict::Ok))
        ));
        let (mut job, _release) = slow_job(task());
        tokio::time::timeout(Duration::from_secs(1), job.cancel_and_wait())
            .await
            .unwrap();
        assert!(job.worker.is_finished());
    }

    #[tokio::test]
    async fn changed_task_cancels_review_instead_of_applying_obsolete_outcome() {
        let root = tempfile::tempdir().unwrap();
        coordinator_state_registry_save(
            root.path(),
            &BTreeMap::new(),
            &serde_json::json!({"tasks":[{"id":"REVIEW","state":"blocked"}]}),
        )
        .unwrap();
        let mut state = CoordinatorRunState::new();
        let (job, _release) = slow_job(task());
        state.phase_job = Some(job);
        advance_tasks_native(
            root.path(),
            &Default::default(),
            None,
            None,
            1,
            &mut state,
            None,
        )
        .await
        .unwrap();
        assert!(state.phase_job.is_none());
        assert_eq!(
            coordinator_state_registry_load(root.path(), &BTreeMap::new()).unwrap()["tasks"][0]
                ["state"],
            "blocked"
        );
    }
}

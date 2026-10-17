//! Exercise the real completion, selection and acquisition path without an AI process.
use super::*;
use crate::coordinator::engine::{apply_job_completion_in_registry, JobCompletionInput};
use crate::coordinator::error_normalizer::NormalizerRegistry;
use crate::coordinator::model::TaskRegistry;
use crate::coordinator::rate_limit::ToolThrottleState;
use crate::coordinator::task_selector::{select_next_ready_task, TaskSelectorConfig};
use serde_json::json;

fn git(path: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

struct Fixture {
    root: tempfile::TempDir,
    worker: PathBuf,
}
impl Fixture {
    fn new(committed: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q", "-b", "main"]);
        git(root.path(), &["config", "user.name", "MACC Handoff Test"]);
        git(
            root.path(),
            &["config", "user.email", "handoff@example.invalid"],
        );
        std::fs::write(root.path().join(".gitignore"), ".macc/\n").unwrap();
        std::fs::write(root.path().join("tracked.txt"), "base\n").unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-qm", "base"]);
        let worker = root.path().join(".macc/worktree/worker-01");
        git(
            root.path(),
            &[
                "worktree",
                "add",
                "-qb",
                "ai/tool-original/task",
                worker.to_str().unwrap(),
            ],
        );
        if committed {
            std::fs::write(worker.join("committed.txt"), "completed portion\n").unwrap();
            git(&worker, &["add", "committed.txt"]);
            git(&worker, &["commit", "-qm", "partial implementation"]);
        }
        std::fs::write(worker.join("tracked.txt"), "staged implementation\n").unwrap();
        git(&worker, &["add", "tracked.txt"]);
        std::fs::write(
            worker.join("tracked.txt"),
            "staged implementation\nunstaged continuation\n",
        )
        .unwrap();
        std::fs::write(worker.join("new.bin"), [0, 1, 255, 0]).unwrap();
        Self { root, worker }
    }
    fn snapshot(&self) -> (String, String, String, String, Vec<u8>) {
        (
            git(&self.worker, &["rev-parse", "HEAD"]),
            git(&self.worker, &["branch", "--show-current"]),
            git(&self.worker, &["diff", "--cached", "--binary"]),
            git(&self.worker, &["diff", "--binary"]),
            std::fs::read(self.worker.join("new.bin")).unwrap(),
        )
    }
    fn registry(&self, tool: &str) -> serde_json::Value {
        json!({"tasks":[{
            "id":"HANDOFF", "title":"Continue existing task", "state":"claimed", "tool":tool,
            "worktree":{"worktree_path":self.worker,"branch":"ai/tool-original/task","base_branch":"main"},
            "task_runtime":{"status":"running","current_phase":"dev","active_session_id":"original-tool-session"}
        }],"resource_locks":{}})
    }
}
fn input(code: &str) -> JobCompletionInput {
    JobCompletionInput {
        success: false,
        attempt: 1,
        max_attempts: 1,
        timed_out: false,
        phase_timeout_seconds: 0,
        elapsed_seconds: 1,
        status_text: "tool unavailable".into(),
        completion_kind: None,
        error_code: Some(code.into()),
        error_origin: Some("runner".into()),
        error_message: Some("tool unavailable".into()),
        result_explanation: None,
        unmet_preconditions: vec![],
        auto_retry_error_codes: vec!["E101".into()],
        auto_retry_max: 3,
        backoff_base_seconds: 30,
        backoff_max_seconds: 300,
        normalizer_input: None,
    }
}

#[tokio::test]
async fn tool_handoff_preserves_commits_index_working_files_and_untracked_data() {
    for (original, replacement) in [("tool-a", "tool-b"), ("tool-b", "tool-a")] {
        for committed in [false, true] {
            for code in ["E601", "E602", "E101"] {
                let fixture = Fixture::new(committed);
                let before = fixture.snapshot();
                let mut registry = fixture.registry(original);
                let now = "2026-10-03T20:00:00Z";
                let result = apply_job_completion_in_registry(
                    &mut registry,
                    "HANDOFF",
                    &input(code),
                    &NormalizerRegistry::empty(),
                    now,
                )
                .unwrap();
                assert_eq!(registry["tasks"][0]["state"], "todo", "{code}");
                assert_eq!(fixture.snapshot(), before);
                let typed = TaskRegistry::from_value(&registry).unwrap();
                assert!(!typed.can_reuse_worktree_slot(fixture.worker.to_str().unwrap()));
                // Another task must not be able to sanitize the parked slot.
                let (reusable, _) = find_reusable_worktree_native(
                    fixture.root.path(),
                    &registry,
                    replacement,
                    "main",
                    0,
                    &HashMap::new(),
                )
                .unwrap();
                assert!(reusable.is_none());
                assert_eq!(fixture.snapshot(), before);
                // Availability cooldowns must also survive prior implementation
                // retries and a coordinator cleanup/recovery before redispatch.
                if code != "E101" {
                    registry["tasks"][0]["task_runtime"]["retries"] = json!(20);
                }
                // Real persistence round-trip preserves ownership while waiting.
                crate::coordinator::state::coordinator_state_registry_save(
                    fixture.root.path(),
                    &BTreeMap::new(),
                    &registry,
                )
                .unwrap();
                crate::coordinator::state_runtime::cleanup_registry_native(fixture.root.path())
                    .unwrap();
                let recovery = crate::coordinator::state_runtime::execute_startup_recovery_sweep(
                    fixture.root.path(),
                    "main",
                    false,
                    None,
                )
                .unwrap();
                assert!(recovery.is_empty(), "{recovery:?}");
                registry = crate::coordinator::state::coordinator_state_registry_load(
                    fixture.root.path(),
                    &BTreeMap::new(),
                )
                .unwrap();
                assert_eq!(fixture.snapshot(), before);
                let mut config = TaskSelectorConfig {
                    default_tool: original.into(),
                    default_base_branch: "main".into(),
                    tool_priority: vec![original.into(), replacement.into()],
                    enabled_tools: vec![original.into(), replacement.into()],
                    rate_limit_fallback_enabled: true,
                    now: now.into(),
                    max_same_worktree_retries: if code == "E101" { 3 } else { 0 },
                    ..Default::default()
                };
                let throttle: ToolThrottleState = if code == "E101" {
                    ToolThrottleState {
                        tool_id: original.into(),
                        throttled_until: 1791061200,
                        ..Default::default()
                    }
                } else {
                    serde_json::from_value(
                        registry["tasks"][0]["task_runtime"]["throttle_state"].clone(),
                    )
                    .unwrap()
                };
                config.throttle_registry.insert(original.into(), throttle);
                if code == "E602" {
                    let mut availability = CoordinatorRunState::new();
                    let mut canonical = crate::config::CanonicalConfig::default();
                    canonical.tools.enabled = vec![original.into(), replacement.into()];
                    let until = chrono::Utc::now().timestamp() as u64 + 3600;
                    for tool in [original, replacement] {
                        availability.throttle_registry.insert(
                            tool.into(),
                            ToolThrottleState {
                                tool_id: tool.into(),
                                throttled_until: until,
                                ..Default::default()
                            },
                        );
                    }
                    assert!(crate::coordinator::control_plane::waiting_for_tools_native(
                        fixture.root.path(),
                        &canonical,
                        None,
                        &Default::default(),
                        &availability
                    ));
                }
                let selected = select_next_ready_task(&registry, &config)
                    .expect("replacement can resume parked work");
                assert_eq!(selected.id, "HANDOFF");
                assert_eq!(selected.tool, replacement);
                assert_eq!(
                    selected.resume_worktree.as_ref().unwrap().path,
                    fixture.worker.to_str().unwrap()
                );
                let candidate = DispatchCandidate {
                    task: selected,
                    worktree_slot: WorktreeSlot::Auto,
                };
                let mut state = CoordinatorRunState::new();
                let acquired = acquire_worktree_for_dispatch(
                    fixture.root.path(),
                    &registry,
                    &candidate,
                    &CoordinatorConfigResolved::resolve(None),
                    &mut state,
                    None,
                )
                .await
                .unwrap();
                assert_eq!(acquired.path, fixture.worker);
                assert_eq!(acquired.branch, "ai/tool-original/task");
                let claim = claim_task_in_registry(
                    fixture.root.path(),
                    &candidate,
                    &acquired,
                    &mut registry,
                    None,
                )
                .unwrap();
                // Runner config and worktree apply must agree on the new tool.
                std::fs::create_dir_all(fixture.worker.join(".macc")).unwrap();
                std::fs::write(
                    fixture.worker.join(".macc/tool.json"),
                    json!({"id":replacement}).to_string(),
                )
                .unwrap();
                std::fs::write(fixture.worker.join(".macc/worktree.json"), json!({"id":"worker-01","slug":"worker-01","tool":original,"scope":null,"feature":null,"base":"main","branch":"ai/tool-original/task"}).to_string()).unwrap();
                crate::coordinator::control_plane::phase_runner::ensure_tool_json_for_tool(
                    fixture.root.path(),
                    &fixture.worker,
                    replacement,
                )
                .unwrap();
                let metadata = crate::read_worktree_metadata(&fixture.worker)
                    .unwrap()
                    .unwrap();
                assert_eq!(metadata.tool, replacement);
                assert_eq!(metadata.branch, "ai/tool-original/task");
                assert_eq!(claim.tool, replacement);
                assert!(claim.resume_attempt > 0);
                assert_eq!(fixture.snapshot(), before);
                assert_eq!(
                    registry["tasks"][0]["task_runtime"]["last_session_tool"],
                    original
                );
                assert!(registry["tasks"][0]["task_runtime"]["active_session_id"].is_null());
                if code == "E602" {
                    assert_eq!(result.status_label, "quota_exhausted_requeue");
                }
            }
        }
    }
}

#[tokio::test]
async fn unsafe_resume_never_acquires_a_fresh_slot() {
    for locked in [false, true] {
        let fixture = Fixture::new(false);
        let before = fixture.snapshot();
        let mut registry = fixture.registry("tool-a");
        if locked {
            let lock = git(&fixture.worker, &["rev-parse", "--git-path", "index.lock"]);
            std::fs::write(lock.trim(), "incomplete git operation").unwrap();
            apply_job_completion_in_registry(
                &mut registry,
                "HANDOFF",
                &input("E602"),
                &NormalizerRegistry::empty(),
                "2026-10-03T20:00:00Z",
            )
            .unwrap();
            assert_eq!(
                registry["tasks"][0]["worktree"]["worktree_path"],
                fixture.worker.to_str().unwrap()
            );
        }
        let candidate = DispatchCandidate {
            task: crate::coordinator::task_selector::SelectedTask {
                id: "HANDOFF".into(),
                title: "Resume".into(),
                tool: "tool-b".into(),
                base_branch: "main".into(),
                is_fallback: true,
                resume_worktree: Some(crate::coordinator::task_selector::ResumeWorktree {
                    path: fixture.worker.to_string_lossy().into(),
                    branch: if locked {
                        "ai/tool-original/task"
                    } else {
                        "wrong-branch"
                    }
                    .into(),
                }),
            },
            worktree_slot: WorktreeSlot::Auto,
        };
        let mut state = CoordinatorRunState::new();
        let error = acquire_worktree_for_dispatch(
            fixture.root.path(),
            &registry,
            &candidate,
            &CoordinatorConfigResolved::resolve(None),
            &mut state,
            None,
        )
        .await
        .err()
        .unwrap();
        assert!(error
            .to_string()
            .to_ascii_lowercase()
            .contains("existing work was preserved"));
        assert_eq!(fixture.snapshot(), before);
    }
}

#[test]
fn exhausted_retry_budget_keeps_uncommitted_work_for_operator_recovery() {
    let fixture = Fixture::new(false);
    let before = fixture.snapshot();
    let mut registry = fixture.registry("tool-a");
    registry["tasks"][0]["state"] = json!("todo");
    registry["tasks"][0]["task_runtime"]["status"] = json!("failed");
    registry["tasks"][0]["task_runtime"]["retries"] = json!(20);
    registry["tasks"][0]["task_runtime"]["last_error_code"] = json!("E101");
    crate::coordinator::state::coordinator_state_registry_save(
        fixture.root.path(),
        &BTreeMap::new(),
        &registry,
    )
    .unwrap();
    let entries = crate::coordinator::state_runtime::execute_startup_recovery_sweep(
        fixture.root.path(),
        "main",
        false,
        None,
    )
    .unwrap();
    assert_eq!(
        entries[0].classification,
        "parked_unschedulable_with_changes"
    );
    let saved = crate::coordinator::state::coordinator_state_registry_load(
        fixture.root.path(),
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(saved["tasks"][0]["state"], "blocked");
    assert_eq!(
        saved["tasks"][0]["worktree"]["worktree_path"],
        fixture.worker.to_str().unwrap()
    );
    let typed = TaskRegistry::from_value(&saved).unwrap();
    assert!(!typed.can_reuse_worktree_slot(fixture.worker.to_str().unwrap()));
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn crashed_performer_without_terminal_result_retains_uncommitted_work() {
    let fixture = Fixture::new(false);
    let before = fixture.snapshot();
    let mut typed = TaskRegistry::from_value(&fixture.registry("tool-a")).unwrap();
    typed.find_task_mut("HANDOFF").unwrap().task_runtime.pid = Some(999999);
    crate::coordinator::state_runtime::cleanup_dead_runtime_tasks_in_typed_registry(
        &mut typed,
        "handoff regression",
        0,
        None,
        Some(fixture.root.path()),
    )
    .unwrap();
    let task = typed.find_task("HANDOFF").unwrap();
    assert_eq!(task.state, "blocked");
    assert_eq!(task.worktree_path(), Some(fixture.worker.to_str().unwrap()));
    assert!(!typed.can_reuse_worktree_slot(fixture.worker.to_str().unwrap()));
    assert_eq!(fixture.snapshot(), before);
}

#[path = "phase_handoff_tests.rs"]
mod phase_handoff_tests;

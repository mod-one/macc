use super::*;
use crate::commands::AppContext;
use crate::test_support::{run_git_ok, run_git_ok_with_env};
use axum::body::Body;
use axum::http::Request;
use axum::response::IntoResponse;
use http_body_util::BodyExt;
use macc_core::config::CanonicalConfig;
use macc_core::config::WebAssetsMode;
use macc_core::coordinator::model::{Task, TaskRegistry, TaskWorktree};
use macc_core::coordinator::task_selector::SelectedTask;
use macc_core::coordinator::COORDINATOR_EVENT_SCHEMA_VERSION;
use macc_core::coordinator::{CoordinatorEventPayload, CoordinatorEventRecord, RuntimeStatus};
use macc_core::coordinator_storage::{
    CoordinatorSnapshot, CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage,
};
use macc_core::engine::CoordinatorEvent;
use macc_core::resolve::CliOverrides;
use macc_core::service::coordinator_workflow::{
    CoordinatorCommand, CoordinatorCommandRequest, CoordinatorCommandResult, CoordinatorStatus,
};
use macc_core::TestEngine;
use macc_core::{MaccError, ProjectPaths, Result};
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tower::util::ServiceExt;

fn temp_root(label: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("macc-web-{}-{}", label, nanos))
}

fn write_test_config(root: &std::path::Path) {
    let paths = ProjectPaths::from_root(root);
    fs::create_dir_all(paths.config_path.parent().expect("config dir")).expect("mkdir config");
    let yaml = CanonicalConfig::default()
        .to_yaml()
        .expect("serialize config");
    fs::write(&paths.config_path, yaml).expect("write config");
}

fn write_test_config_with_port(root: &std::path::Path, port: u16) {
    let paths = ProjectPaths::from_root(root);
    fs::create_dir_all(paths.config_path.parent().expect("config dir")).expect("mkdir config");
    let mut canonical = CanonicalConfig::default();
    canonical.settings.web_port = Some(port);
    let yaml = canonical.to_yaml().expect("serialize config");
    fs::write(&paths.config_path, yaml).expect("write config");
}

fn write_test_config_with_assets_mode(root: &std::path::Path, assets_mode: WebAssetsMode) {
    let paths = ProjectPaths::from_root(root);
    fs::create_dir_all(paths.config_path.parent().expect("config dir")).expect("mkdir config");
    let mut canonical = CanonicalConfig::default();
    canonical.settings.web_assets = Some(assets_mode);
    let yaml = canonical.to_yaml().expect("serialize config");
    fs::write(&paths.config_path, yaml).expect("write config");
}

fn write_test_dist_assets(root: &std::path::Path) {
    let dist_dir = root.join("web").join("dist").join("assets");
    fs::create_dir_all(&dist_dir).expect("mkdir dist assets");
    fs::write(
        root.join("web").join("dist").join("index.html"),
        "<!doctype html><html><body>dist web</body></html>",
    )
    .expect("write index");
    fs::write(dist_dir.join("app.js"), "console.log('dist');").expect("write asset");
}

fn init_git_repo(root: &Path) {
    fs::create_dir_all(root).expect("create repo root");
    run_git_ok(root, &["init", "-b", "main"]);
    run_git_ok(root, &["config", "user.name", "MACC Test"]);
    run_git_ok(root, &["config", "user.email", "macc@example.com"]);
}

fn commit_file(
    root: &Path,
    file_name: &str,
    contents: &str,
    subject: &str,
    body: &str,
    timestamp: &str,
) {
    fs::write(root.join(file_name), contents).expect("write file");
    run_git_ok(root, &["add", file_name]);
    let envs = [
        ("GIT_AUTHOR_DATE", timestamp),
        ("GIT_COMMITTER_DATE", timestamp),
    ];
    if body.is_empty() {
        run_git_ok_with_env(root, &["commit", "-m", subject], &envs);
    } else {
        run_git_ok_with_env(root, &["commit", "-m", subject, "-m", body], &envs);
    }
}

fn merge_branch(root: &Path, branch: &str, subject: &str, body: &str, timestamp: &str) {
    let envs = [
        ("GIT_AUTHOR_DATE", timestamp),
        ("GIT_COMMITTER_DATE", timestamp),
    ];
    if body.is_empty() {
        run_git_ok_with_env(root, &["merge", "--no-ff", branch, "-m", subject], &envs);
    } else {
        run_git_ok_with_env(
            root,
            &["merge", "--no-ff", branch, "-m", subject, "-m", body],
            &envs,
        );
    }
}

fn build_git_graph_repo(root: &Path) {
    init_git_repo(root);
    commit_file(
        root,
        "base.txt",
        "base\n",
        "feat: WEB2-BE-GIT-001 - base commit",
        "[macc:task WEB2-BE-GIT-001]",
        "2026-03-18T10:00:00Z",
    );
    run_git_ok(root, &["checkout", "-b", "feature/web-graph"]);
    commit_file(
        root,
        "feature.txt",
        "feature\n",
        "feat: WEB2-BE-GIT-002 - feature commit",
        "[macc:task WEB2-BE-GIT-002]",
        "2026-03-18T11:00:00Z",
    );
    run_git_ok(root, &["checkout", "main"]);
    commit_file(
        root,
        "main.txt",
        "main\n",
        "chore: WEB2-BE-GIT-003 - main commit",
        "[macc:task WEB2-BE-GIT-003]",
        "2026-03-18T12:00:00Z",
    );
    merge_branch(
        root,
        "feature/web-graph",
        "macc: merge task WEB2-BE-GIT-002",
        "[macc:task WEB2-BE-GIT-002]\n[macc:merge true]",
        "2026-03-18T13:00:00Z",
    );
}

fn test_web_state(
    root: &std::path::Path,
    engine: SharedEngine,
    assets_mode: WebAssetsMode,
) -> WebState {
    WebState {
        engine,
        paths: ProjectPaths::from_root(root),
        assets_mode,
    }
}

fn write_registry_snapshot(root: &Path, snapshot: &CoordinatorSnapshot) {
    let project_paths = ProjectPaths::from_root(root);
    let storage_paths = CoordinatorStoragePaths::from_project_paths(&project_paths);
    SqliteStorage::new(storage_paths)
        .save_snapshot(snapshot)
        .expect("save registry snapshot");
}

fn registry_task(id: &str, title: &str, state: &str, tool: Option<&str>) -> Task {
    Task {
        id: id.to_string(),
        title: Some(title.to_string()),
        priority: Some("P1".to_string()),
        state: state.to_string(),
        tool: tool.map(ToString::to_string),
        ..Task::default()
    }
}

fn registry_event(task_id: &str, event_id: &str, event_type: &str) -> CoordinatorEventRecord {
    CoordinatorEventRecord {
        schema_version: COORDINATOR_EVENT_SCHEMA_VERSION.to_string(),
        event_id: event_id.to_string(),
        run_id: Some("run-1".to_string()),
        seq: 1,
        ts: "2026-03-20T12:00:00Z".to_string(),
        source: "coordinator:web".to_string(),
        task_id: Some(task_id.to_string()),
        event_type: event_type.to_string(),
        phase: Some("dev".to_string()),
        status: "ok".to_string(),
        detail: Some(format!("{event_type} for {task_id}")),
        msg: None,
        payload: CoordinatorEventPayload::from(serde_json::json!({
            "message": format!("{event_type} for {task_id}")
        }))
        .into_value(),
        extra: Default::default(),
    }
}

struct WebTestEngine {
    inner: TestEngine,
    run_result: std::sync::Mutex<Option<std::result::Result<CoordinatorCommandResult, MaccError>>>,
    cleanup_result: std::sync::Mutex<Option<std::result::Result<(), MaccError>>>,
    stop_result: std::sync::Mutex<Option<std::result::Result<(), MaccError>>>,
    resume_result: std::sync::Mutex<Option<std::result::Result<(), MaccError>>>,
    coordinator_events: std::sync::Mutex<Vec<Vec<CoordinatorEvent>>>,
}

impl WebTestEngine {
    fn new(result: std::result::Result<CoordinatorCommandResult, MaccError>) -> Self {
        Self {
            inner: TestEngine::with_fixtures(),
            run_result: std::sync::Mutex::new(Some(result)),
            cleanup_result: std::sync::Mutex::new(Some(Ok(()))),
            stop_result: std::sync::Mutex::new(Some(Ok(()))),
            resume_result: std::sync::Mutex::new(Some(Ok(()))),
            coordinator_events: std::sync::Mutex::new(vec![Vec::new()]),
        }
    }

    fn with_cleanup_result(result: std::result::Result<(), MaccError>) -> Self {
        Self {
            inner: TestEngine::with_fixtures(),
            run_result: std::sync::Mutex::new(Some(Ok(CoordinatorCommandResult::default()))),
            cleanup_result: std::sync::Mutex::new(Some(result)),
            stop_result: std::sync::Mutex::new(Some(Ok(()))),
            resume_result: std::sync::Mutex::new(Some(Ok(()))),
            coordinator_events: std::sync::Mutex::new(vec![Vec::new()]),
        }
    }

    fn with_stop_result(result: std::result::Result<(), MaccError>) -> Self {
        Self {
            inner: TestEngine::with_fixtures(),
            run_result: std::sync::Mutex::new(Some(Ok(CoordinatorCommandResult::default()))),
            cleanup_result: std::sync::Mutex::new(Some(Ok(()))),
            stop_result: std::sync::Mutex::new(Some(result)),
            resume_result: std::sync::Mutex::new(Some(Ok(()))),
            coordinator_events: std::sync::Mutex::new(vec![Vec::new()]),
        }
    }

    fn with_resume_result(result: std::result::Result<(), MaccError>) -> Self {
        Self {
            inner: TestEngine::with_fixtures(),
            run_result: std::sync::Mutex::new(Some(Ok(CoordinatorCommandResult::default()))),
            cleanup_result: std::sync::Mutex::new(Some(Ok(()))),
            stop_result: std::sync::Mutex::new(Some(Ok(()))),
            resume_result: std::sync::Mutex::new(Some(result)),
            coordinator_events: std::sync::Mutex::new(vec![Vec::new()]),
        }
    }

    fn with_event_snapshots(event_snapshots: Vec<Vec<CoordinatorEvent>>) -> Self {
        Self {
            inner: TestEngine::with_fixtures(),
            run_result: std::sync::Mutex::new(Some(Ok(CoordinatorCommandResult::default()))),
            cleanup_result: std::sync::Mutex::new(Some(Ok(()))),
            stop_result: std::sync::Mutex::new(Some(Ok(()))),
            resume_result: std::sync::Mutex::new(Some(Ok(()))),
            coordinator_events: std::sync::Mutex::new(event_snapshots),
        }
    }
}

impl macc_core::engine::Engine for WebTestEngine {
    fn list_tools(
        &self,
        paths: &ProjectPaths,
    ) -> (
        Vec<macc_core::ToolDescriptor>,
        Vec<macc_core::tool::ToolDiagnostic>,
    ) {
        self.inner.list_tools(paths)
    }

    fn doctor(&self, paths: &ProjectPaths) -> Vec<macc_core::doctor::ToolCheck> {
        self.inner.doctor(paths)
    }

    fn plan(
        &self,
        paths: &ProjectPaths,
        config: &macc_core::config::CanonicalConfig,
        materialized_units: &[macc_core::resolve::MaterializedFetchUnit],
        overrides: &macc_core::resolve::CliOverrides,
    ) -> Result<macc_core::plan::ActionPlan> {
        self.inner
            .plan(paths, config, materialized_units, overrides)
    }

    fn plan_operations(
        &self,
        paths: &ProjectPaths,
        plan: &macc_core::plan::ActionPlan,
    ) -> Vec<macc_core::plan::PlannedOp> {
        self.inner.plan_operations(paths, plan)
    }

    fn apply(
        &self,
        paths: &ProjectPaths,
        plan: &mut macc_core::plan::ActionPlan,
        allow_user_scope: bool,
    ) -> Result<macc_core::ApplyReport> {
        self.inner.apply(paths, plan, allow_user_scope)
    }

    fn builtin_skills(&self) -> Vec<macc_core::catalog::Skill> {
        self.inner.builtin_skills()
    }

    fn builtin_agents(&self) -> Vec<macc_core::catalog::Agent> {
        self.inner.builtin_agents()
    }

    fn coordinator_execute_command(
        &self,
        _paths: &ProjectPaths,
        _command: CoordinatorCommand,
        _request: CoordinatorCommandRequest<'_>,
    ) -> Result<CoordinatorCommandResult> {
        self.run_result
            .lock()
            .expect("lock")
            .take()
            .unwrap_or_else(|| Ok(CoordinatorCommandResult::default()))
    }

    fn coordinator_stop(&self, _repo_root: &std::path::Path, _reason: &str) -> Result<()> {
        self.stop_result
            .lock()
            .expect("lock")
            .take()
            .unwrap_or_else(|| Ok(()))
    }

    fn coordinator_cleanup(&self, _paths: &ProjectPaths) -> Result<()> {
        self.cleanup_result
            .lock()
            .expect("lock")
            .take()
            .unwrap_or_else(|| Ok(()))
    }

    fn coordinator_resume(&self, _repo_root: &std::path::Path) -> Result<()> {
        self.resume_result
            .lock()
            .expect("lock")
            .take()
            .unwrap_or_else(|| Ok(()))
    }

    fn get_coordinator_events(&self, _paths: &ProjectPaths) -> Result<Vec<CoordinatorEvent>> {
        let mut snapshots = self.coordinator_events.lock().expect("lock");
        let snapshot = if snapshots.len() > 1 {
            snapshots.remove(0)
        } else {
            snapshots.first().cloned().unwrap_or_default()
        };
        Ok(snapshot)
    }
}

fn coordinator_event(seq: i64, event_id: &str, event_type: &str) -> CoordinatorEvent {
    CoordinatorEvent {
        event_id: Some(event_id.to_string()),
        run_id: Some("run-1".to_string()),
        seq,
        event_type: event_type.to_string(),
        task_id: Some("WEB-BACKEND-008".to_string()),
        phase: Some("implement".to_string()),
        status: Some("ok".to_string()),
        ts: Some("2026-03-19T12:00:00Z".to_string()),
        message: None,
        raw: serde_json::json!({
            "schema_version": COORDINATOR_EVENT_SCHEMA_VERSION,
            "event_id": event_id,
            "run_id": "run-1",
            "seq": seq,
            "ts": "2026-03-19T12:00:00Z",
            "source": "coordinator",
            "task_id": "WEB-BACKEND-008",
            "type": event_type,
            "phase": "implement",
            "status": "ok",
        }),
    }
}

include!("tests_body.inc");

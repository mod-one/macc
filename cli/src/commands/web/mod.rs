#[allow(clippy::result_large_err)]
mod apply;
mod assets;
mod audit;
#[allow(clippy::result_large_err)]
mod backups;
mod config;
mod coordinator;
mod failures;
mod search;
mod catalog_skills;
mod skill_runs;
mod snapshot;
#[allow(clippy::result_large_err)]
mod doctor;
mod errors;
#[allow(clippy::result_large_err)]
mod git;
#[allow(clippy::result_large_err)]
mod logs;
mod mutation_gate;
mod ownership;
mod plan;
#[allow(clippy::result_large_err)]
mod prd;
#[allow(clippy::result_large_err)]
mod registry;
mod sse;
mod task_endpoints;
#[allow(clippy::result_large_err)]
mod terminal;
#[cfg(test)]
mod tests;
mod types;
#[allow(clippy::result_large_err)]
mod worktrees;

use crate::commands::AppContext;
use crate::commands::Command;
use crate::services::engine_provider::SharedEngine;
use axum::middleware::from_fn_with_state;
use axum::routing::{delete, get, post, put};
use axum::Json;
use axum::Router;
use macc_core::config::WebAssetsMode;
use macc_core::process_ownership::{ClientKind, ProcessHandle, ProcessKind};
use macc_core::service::process_ownership::RegisteredProcessGuard;
use macc_core::{MaccError, ProjectPaths, Result};
use macc_core::coordinator_storage::CoordinatorStorage;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

const WEB_VIEWER_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

pub struct WebCommand {
    app: AppContext,
    host: String,
    port: Option<u16>,
    assets_mode: Option<WebAssetsMode>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WebServerConfig {
    host: IpAddr,
    port: u16,
    assets_mode: WebAssetsMode,
}

#[derive(Clone)]
struct WebState {
    engine: SharedEngine,
    paths: ProjectPaths,
    assets_mode: WebAssetsMode,
    tail_stream_limiter: logs::TailStreamLimiter,
    terminal_sessions: terminal::TerminalSessionStore,
    #[allow(dead_code)]
    registered_process_guard: Option<Arc<RegisteredProcessGuard>>,
}

impl WebCommand {
    pub fn new(
        app: AppContext,
        host: String,
        port: Option<u16>,
        assets_mode: Option<WebAssetsMode>,
    ) -> Self {
        Self {
            app,
            host,
            port,
            assets_mode,
        }
    }

    fn server_config(&self) -> Result<WebServerConfig> {
        let canonical = self.app.canonical_config()?;
        let host = self.host.parse::<IpAddr>().map_err(|e| {
            MaccError::Validation(format!("invalid web host '{}': {}", self.host, e))
        })?;
        Ok(WebServerConfig {
            host,
            port: self
                .port
                .unwrap_or(canonical.settings.web_port.unwrap_or(3450)),
            assets_mode: self.assets_mode.unwrap_or_else(|| {
                canonical
                    .settings
                    .web_assets
                    .unwrap_or_else(default_web_assets_mode)
            }),
        })
    }
}

impl Command for WebCommand {
    fn run(&self) -> Result<()> {
        let config = self.server_config()?;
        let paths = self.app.project_paths()?;

        let handle = ProcessHandle {
            kind: ProcessKind::WebServer,
            project_root: paths.root.clone(),
            pid: Some(std::process::id() as i32),
        };
        let guard = self.app.engine.process_register(&paths.root, handle)?;

        let state = WebState {
            engine: self.app.engine.clone(),
            paths,
            assets_mode: config.assets_mode,
            tail_stream_limiter: logs::TailStreamLimiter::default(),
            terminal_sessions: terminal::TerminalSessionStore::default(),
            registered_process_guard: Some(Arc::new(guard)),
        };
        let app = build_web_router(state.clone());

        println!("Web server starting on http://{}...", config.bind_addr());

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| MaccError::Validation(format!("build web runtime: {}", e)))?;

        runtime.block_on(async move {
            let addr = config.bind_addr();
            let listener =
                tokio::net::TcpListener::bind(addr)
                    .await
                    .map_err(|e| MaccError::Io {
                        path: addr.to_string(),
                        action: "bind web server".into(),
                        source: e,
                    })?;
            tokio::spawn(run_web_viewer_heartbeat_loop(state));
            axum::serve(listener, app)
                .await
                .map_err(|e| MaccError::Validation(format!("web server failed: {}", e)))
        })?;

        Ok(())
    }
}

impl WebServerConfig {
    fn bind_addr(self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }
}

fn build_web_router(state: WebState) -> Router {
    let audit_state = state.clone();
    Router::new()
        .route("/api/v1/health", get(health_handler))
        .route("/api/v1/trust", get(trust_handler))
        .route("/api/v1/doctor", get(doctor::get_doctor_handler))
        .route("/api/v1/doctor/fix", post(doctor::run_doctor_fix_handler))
        .route(
            "/api/v1/config",
            get(config::get_config_handler).put(config::update_config_handler),
        )
        .route(
            "/api/v1/config/tool-descriptors",
            get(config::get_tool_descriptors_handler),
        )
        .route(
            "/api/v1/config/standards-preview",
            post(config::standards_preview_handler),
        )
        .route("/api/v1/plan", post(plan::run_plan_handler))
        .route("/api/v1/apply", post(apply::run_apply_handler))
        .route("/api/v1/status", get(coordinator::status_handler))
        .route("/api/v1/git/graph", get(git::get_git_graph_handler))
        .route("/api/v1/logs", get(logs::list_logs_handler))
        .route("/api/v1/logs/tail", get(logs::tail_log_handler))
        .route("/api/v1/logs/*path", get(logs::read_log_handler))
        .route("/api/v1/backups", get(backups::list_backups_handler))
        .route(
            "/api/v1/backups/:id/restore",
            post(backups::restore_backup_handler),
        )
        .route(
            "/api/v1/worktrees",
            get(worktrees::list_worktrees_handler).post(worktrees::create_worktree_handler),
        )
        .route(
            "/api/v1/worktrees/:id",
            delete(worktrees::delete_worktree_handler),
        )
        .route(
            "/api/v1/worktrees/:id/run",
            post(worktrees::run_worktree_handler),
        )
        .route(
            "/api/v1/worktrees/:id/logs",
            get(worktrees::worktree_logs_handler),
        )
        .route("/api/v1/events", get(sse::events_handler))
        .route("/api/v1/terminal", post(terminal::create_terminal_handler))
        .route(
            "/api/v1/terminal/:session",
            get(terminal::terminal_ws_handler),
        )
        .route(
            "/api/v1/coordinator/run",
            post(coordinator::coordinator_run_handler),
        )
        .route(
            "/api/v1/coordinator/dispatch",
            post(coordinator::coordinator_dispatch_handler),
        )
        .route(
            "/api/v1/coordinator/advance",
            post(coordinator::coordinator_advance_handler),
        )
        .route(
            "/api/v1/coordinator/reconcile",
            post(coordinator::coordinator_reconcile_handler),
        )
        .route(
            "/api/v1/coordinator/cleanup",
            post(coordinator::coordinator_cleanup_handler),
        )
        .route(
            "/api/v1/coordinator/stop",
            post(coordinator::coordinator_stop_handler),
        )
        .route(
            "/api/v1/coordinator/processes",
            get(coordinator::coordinator_processes_handler),
        )
        .route(
            "/api/v1/coordinator/recover",
            post(coordinator::coordinator_recover_handler),
        )
        .route(
            "/api/v1/coordinator/resume",
            post(coordinator::coordinator_resume_handler),
        )
        .route(
            "/api/v1/coordinator/sync",
            post(coordinator::coordinator_sync_handler),
        )
        .route(
            "/api/v1/coordinator/audit-prd",
            post(coordinator::coordinator_audit_prd_handler),
        )
        .route(
            "/api/v1/coordinator/preflight",
            post(coordinator::coordinator_preflight_handler),
        )
        .route(
            "/api/v1/coordinator/preflight/create-reference-branch",
            post(coordinator::coordinator_preflight_create_branch_handler),
        )
        .route(
            "/api/v1/coordinator/tool-cooldown",
            get(coordinator::get_tool_cooldowns_handler)
                .post(coordinator::set_tool_cooldown_handler),
        )
        .route(
            "/api/v1/coordinator/tool-cooldown/:tool",
            delete(coordinator::clear_tool_cooldown_handler),
        )
        .route(
            "/api/v1/registry/tasks",
            get(registry::list_registry_tasks_handler),
        )
        .route(
            "/api/v1/registry/tasks/:id/:action",
            post(registry::task_action_handler),
        )
        .route(
            "/api/v1/registry/tasks/:id",
            get(task_endpoints::get_registry_task_handler),
        )
        .route(
            "/api/v1/registry/tasks/:id/events",
            get(task_endpoints::get_registry_task_events_handler),
        )
        .route(
            "/api/v1/registry/tasks/:id/logs",
            get(task_endpoints::get_registry_task_logs_handler),
        )
        .route(
            "/api/v1/registry/tasks/:id/diff",
            get(task_endpoints::get_registry_task_diff_handler),
        )
        .route(
            "/api/v1/registry/tasks/:id/explain",
            get(task_endpoints::get_registry_task_explain_handler),
        )
        .route(
            "/api/v1/tasks/:id/stream",
            get(task_endpoints::task_stream_handler),
        )
        .route(
            "/api/v1/registry/tasks/:id/retry",
            post(task_endpoints::task_retry_handler),
        )
        .route(
            "/api/v1/registry/tasks/:id/stop",
            post(task_endpoints::task_stop_handler),
        )
        .route(
            "/api/v1/registry/tasks/:id/run-testing",
            post(task_endpoints::task_run_testing_handler),
        )
        .route(
            "/api/v1/registry/tasks/:id/run-review",
            post(task_endpoints::task_run_review_handler),
        )
        .route("/api/v1/prd", get(prd::get_prd_handler))
        .route("/api/v1/prd", put(prd::update_prd_handler))
        .route("/api/v1/processes", get(ownership::list_processes_handler))
        .route(
            "/api/v1/processes/:kind/ownership",
            get(ownership::get_process_ownership_handler),
        )
        .route(
            "/api/v1/processes/:kind/claim",
            post(ownership::claim_ownership_handler),
        )
        .route(
            "/api/v1/processes/:kind/release",
            post(ownership::release_ownership_handler),
        )
        .route(
            "/api/v1/processes/:kind/viewer",
            post(ownership::add_viewer_handler).delete(ownership::remove_viewer_handler),
        )
        .route(
            "/api/v1/processes/:kind/takeover/request",
            post(ownership::request_takeover_handler),
        )
        .route(
            "/api/v1/processes/:kind/takeover/respond",
            post(ownership::respond_takeover_handler),
        )
        .route(
            "/api/v1/processes/:kind/heartbeat",
            post(ownership::heartbeat_handler),
        )
        // ── UX observability endpoints (spec §4.21, §8) ─────────────────
        .route("/api/v1/snapshot", get(snapshot::get_snapshot_handler))
        .route("/api/v1/search", get(search::search_handler))
        // ── Catalog skills lifecycle (spec §16) ──────────────────────────
        .route("/api/v1/catalog/skills/available", get(catalog_skills::available_handler))
        .route("/api/v1/catalog/skills/status", get(catalog_skills::status_handler))
        .route("/api/v1/catalog/skills/installed", get(catalog_skills::installed_handler))
        .route("/api/v1/catalog/skills/verify", post(catalog_skills::verify_handler))
        .route("/api/v1/catalog/skills/lockfile", get(catalog_skills::lockfile_handler))
        .route("/api/v1/skills", get(skill_runs::list_skills_handler))
        .route("/api/v1/skills/:id", get(skill_runs::get_skill_handler))
        .route(
            "/api/v1/skills/:id/dry-run",
            post(skill_runs::dry_run_skill_handler),
        )
        .route(
            "/api/v1/skills/:id/run",
            post(skill_runs::run_skill_handler),
        )
        .route("/api/v1/runs", get(skill_runs::list_runs_handler))
        .route("/api/v1/runs/:id", get(skill_runs::get_run_handler))
        .route(
            "/api/v1/runs/:id/logs",
            get(skill_runs::get_run_logs_handler),
        )
        .route(
            "/api/v1/failures/recent",
            get(failures::recent_failures_handler),
        )
        .route(
            "/api/v1/workers/:id/snapshot",
            get(snapshot::get_worker_snapshot_handler),
        )
        .fallback(get(assets::spa_handler))
        .layer(from_fn_with_state(audit_state, audit::audit_middleware))
        .with_state(state)
}

async fn run_web_viewer_heartbeat_loop(state: WebState) {
    let mut tick = tokio::time::interval(WEB_VIEWER_HEARTBEAT_INTERVAL);
    tick.tick().await;

    loop {
        tick.tick().await;
        heartbeat_web_viewers_once(&state);
    }
}

fn heartbeat_web_viewers_once(state: &WebState) {
    let records = match state.engine.process_list_running(&state.paths.root) {
        Ok(records) => records,
        Err(err) => {
            tracing::warn!(
                "failed to list process ownership records for web heartbeats: {}",
                err
            );
            return;
        }
    };

    for record in records {
        let handle = record.process.clone();
        for viewer in record.viewers {
            if viewer.kind != ClientKind::Web {
                continue;
            }
            if let Err(err) =
                state
                    .engine
                    .process_heartbeat(&state.paths.root, &handle, &viewer.client_id)
            {
                tracing::warn!(
                    process_kind = ?handle.kind,
                    pid = handle.pid,
                    client_id = viewer.client_id,
                    "failed to refresh web viewer heartbeat: {}",
                    err
                );
            }
        }
    }
}

async fn health_handler(
    axum::extract::State(state): axum::extract::State<WebState>,
) -> Json<serde_json::Value> {
    let paths = state.paths.clone();
    let status_res = tokio::task::spawn_blocking(move || {
        let storage_paths = macc_core::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&paths);
        let sqlite = macc_core::coordinator_storage::SqliteStorage::new(storage_paths);
        
        let db_ok = sqlite.load_snapshot().is_ok();
        let last_run = sqlite.get_active_coordinator_run().unwrap_or(None);
        let active = last_run.as_ref().map(|r| r.status == "running").unwrap_or(false);
        let mode = last_run.as_ref().map(|r| r.status.clone()).unwrap_or_else(|| "stopped".to_string());
        let run_id = last_run.as_ref().map(|r| r.run_id.clone()).unwrap_or_default();
        let last_tick_at = last_run.as_ref().and_then(|r| r.last_tick_at.clone()).unwrap_or_default();
        let started_at = last_run.as_ref().map(|r| r.started_at.clone()).unwrap_or_default();
        
        let snapshot = sqlite.load_snapshot().ok();
        let (active_tasks, stale_tasks, blocked_tasks) = if let Some(ref snap) = snapshot {
            let mut active_count = 0;
            let mut stale_count = 0;
            let mut blocked_count = 0;
            for t in &snap.registry.tasks {
                if t.is_active() {
                    active_count += 1;
                    if let Some(lh) = t.task_runtime.last_heartbeat.as_deref() {
                        if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(lh) {
                            let elapsed = chrono::Utc::now().timestamp() - parsed.with_timezone(&chrono::Utc).timestamp();
                            if elapsed > 180 {
                                stale_count += 1;
                            }
                        }
                    }
                } else if t.state == "blocked" {
                    blocked_count += 1;
                }
            }
            (active_count, stale_count, blocked_count)
        } else {
            (0, 0, 0)
        };
        
        let db_pids: Vec<i64> = if let Some(ref snap) = snapshot {
            snap.registry.tasks.iter().filter_map(|t| t.runtime_pid()).collect()
        } else {
            Vec::new()
        };
        
        let orphaned_processes = if db_ok {
            let mut candidate_pids = std::collections::HashSet::new();
            if let Ok(pids) = macc_core::coordinator::helpers::pgrep_pids("performer") {
                for pid in pids { candidate_pids.insert(pid); }
            }
            if let Ok(pids) = macc_core::coordinator::helpers::pgrep_pids("macc") {
                for pid in pids { candidate_pids.insert(pid); }
            }
            let mut count = 0;
            let current_pid = std::process::id() as i32;
            for pid in candidate_pids {
                if pid == current_pid { continue; }
                if db_pids.contains(&(pid as i64)) { continue; }
                if macc_core::coordinator::helpers::pid_in_repo(pid, &paths.root) {
                    count += 1;
                }
            }
            count
        } else {
            0
        };

        let mut status = "ok";
        if stale_tasks > 0 || orphaned_processes > 0 {
            status = "degraded";
        }
        if !db_ok {
            status = "unhealthy";
        }

        (status, db_ok, active, mode, run_id, last_tick_at, active_tasks, stale_tasks, blocked_tasks, orphaned_processes, started_at)
    })
    .await;

    let (status, db_ok, active, mode, run_id, last_tick_at, active_tasks, stale_tasks, blocked_tasks, orphaned_processes, started_at) = match status_res {
        Ok(val) => val,
        Err(_) => ("unhealthy", false, false, "stopped".to_string(), "".to_string(), "".to_string(), 0, 0, 0, 0, "".to_string()),
    };

    let uptime_seconds = if !started_at.is_empty() {
        if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(&started_at) {
            let elapsed = chrono::Utc::now().timestamp() - parsed.with_timezone(&chrono::Utc).timestamp();
            if elapsed > 0 {
                elapsed as u64
            } else {
                0
            }
        } else {
            0
        }
    } else {
        0
    };

    Json(serde_json::json!({
        "status": status,
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_seconds": uptime_seconds,
        "db_ok": db_ok,
        "coordinator": {
            "active": active,
            "mode": mode,
            "run_id": run_id,
            "last_tick_at": last_tick_at,
            "last_heartbeat_at": last_tick_at,
            "active_tasks": active_tasks,
            "stale_tasks": stale_tasks,
            "blocked_tasks": blocked_tasks,
            "orphaned_processes": orphaned_processes
        }
    }))
}

async fn trust_handler(
    axum::extract::State(state): axum::extract::State<WebState>,
) -> std::result::Result<Json<macc_core::ops_motif::TrustSummary>, errors::ApiError> {
    let config = state.engine.load_canonical_config(&state.paths).map_err(errors::ApiError::from)?;
    let trust = macc_core::ops_motif::calculate_trust_summary(&state.paths, &config);
    Ok(Json(trust))
}

#[cfg(debug_assertions)]
fn default_web_assets_mode() -> WebAssetsMode {
    WebAssetsMode::Dist
}

#[cfg(not(debug_assertions))]
fn default_web_assets_mode() -> WebAssetsMode {
    WebAssetsMode::Embedded
}

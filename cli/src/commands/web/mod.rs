#[allow(clippy::result_large_err)]
mod apply;
mod assets;
mod audit;
#[allow(clippy::result_large_err)]
mod backups;
mod config;
mod coordinator;
#[allow(clippy::result_large_err)]
mod doctor;
mod errors;
#[allow(clippy::result_large_err)]
mod git;
#[allow(clippy::result_large_err)]
mod logs;
mod ownership;
mod plan;
#[allow(clippy::result_large_err)]
mod prd;
#[allow(clippy::result_large_err)]
mod registry;
mod sse;
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
use macc_core::process_ownership::{ProcessHandle, ProcessKind};
use macc_core::service::process_ownership::RegisteredProcessGuard;
use macc_core::{MaccError, ProjectPaths, Result};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

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
        let app = build_web_router(state);

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
        .route("/api/v1/prd", get(prd::get_prd_handler))
        .route("/api/v1/prd", put(prd::update_prd_handler))
        .route(
            "/api/v1/processes",
            get(ownership::list_processes_handler),
        )
        .route(
            "/api/v1/processes/:handle/ownership",
            get(ownership::get_process_ownership_handler),
        )
        .route(
            "/api/v1/processes/:handle/claim",
            post(ownership::claim_ownership_handler),
        )
        .route(
            "/api/v1/processes/:handle/release",
            post(ownership::release_ownership_handler),
        )
        .route(
            "/api/v1/processes/:handle/viewer",
            post(ownership::add_viewer_handler).delete(ownership::remove_viewer_handler),
        )
        .route(
            "/api/v1/processes/:handle/takeover/request",
            post(ownership::request_takeover_handler),
        )
        .route(
            "/api/v1/processes/:handle/takeover/respond",
            post(ownership::respond_takeover_handler),
        )
        .route(
            "/api/v1/processes/:handle/heartbeat",
            post(ownership::heartbeat_handler),
        )
        .fallback(get(assets::spa_handler))
        .layer(from_fn_with_state(audit_state, audit::audit_middleware))
        .with_state(state)
}

async fn health_handler(
    axum::extract::State(state): axum::extract::State<WebState>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "project_root": state.paths.root.to_string_lossy()
    }))
}

#[cfg(debug_assertions)]
fn default_web_assets_mode() -> WebAssetsMode {
    WebAssetsMode::Dist
}

#[cfg(not(debug_assertions))]
fn default_web_assets_mode() -> WebAssetsMode {
    WebAssetsMode::Embedded
}

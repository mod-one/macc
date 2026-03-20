mod assets;
mod audit;
mod backups;
mod config;
mod coordinator;
mod errors;
mod git;
mod prd;
mod registry;
mod sse;
#[cfg(test)]
mod tests;
mod types;

use crate::commands::AppContext;
use crate::commands::Command;
use crate::services::engine_provider::SharedEngine;
use axum::middleware::from_fn_with_state;
use axum::routing::{get, post, put};
use axum::Json;
use axum::Router;
use macc_core::config::WebAssetsMode;
use macc_core::{MaccError, ProjectPaths, Result};
use std::net::{IpAddr, SocketAddr};

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
        let state = WebState {
            engine: self.app.engine.clone(),
            paths: self.app.project_paths()?,
            assets_mode: config.assets_mode,
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
        .route(
            "/api/v1/config",
            get(config::get_config_handler).put(config::update_config_handler),
        )
        .route("/api/v1/status", get(coordinator::status_handler))
        .route("/api/v1/git/graph", get(git::get_git_graph_handler))
        .route("/api/v1/backups", get(backups::list_backups_handler))
        .route(
            "/api/v1/backups/:id/restore",
            post(backups::restore_backup_handler),
        )
        .route("/api/v1/events", get(sse::events_handler))
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
            "/api/v1/registry/tasks",
            get(registry::list_registry_tasks_handler),
        )
        .route(
            "/api/v1/registry/tasks/:id/:action",
            post(registry::task_action_handler),
        )
        .route("/api/v1/prd", get(prd::get_prd_handler))
        .route("/api/v1/prd", put(prd::update_prd_handler))
        .fallback(get(assets::spa_handler))
        .layer(from_fn_with_state(audit_state, audit::audit_middleware))
        .with_state(state)
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

#[cfg(debug_assertions)]
fn default_web_assets_mode() -> WebAssetsMode {
    WebAssetsMode::Dist
}

#[cfg(not(debug_assertions))]
fn default_web_assets_mode() -> WebAssetsMode {
    WebAssetsMode::Embedded
}

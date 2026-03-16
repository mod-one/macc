use crate::commands::AppContext;
use crate::commands::Command;
use axum::routing::get;
use axum::Json;
use axum::Router;
use macc_core::MaccError;
use macc_core::Result;
use serde_json::json;
use std::env;
use std::net::SocketAddr;

pub struct WebCommand {
    _app: AppContext,
}

impl WebCommand {
    pub fn new(app: AppContext) -> Self {
        Self { _app: app }
    }
}

impl Command for WebCommand {
    fn run(&self) -> Result<()> {
        run_server()
    }
}

const DEFAULT_WEB_PORT: u16 = 3000;

fn resolve_port() -> Result<u16> {
    match env::var("MACC_WEB_PORT") {
        Ok(raw) => raw.parse::<u16>().map_err(|_| {
            MaccError::Validation(format!("Invalid MACC_WEB_PORT value: {raw}"))
        }),
        Err(env::VarError::NotPresent) => Ok(DEFAULT_WEB_PORT),
        Err(err) => Err(MaccError::Validation(format!(
            "Unable to read MACC_WEB_PORT: {err}"
        ))),
    }
}

async fn root_handler() -> Json<serde_json::Value> {
    Json(json!({ "status": "running" }))
}

#[tokio::main]
async fn run_server() -> Result<()> {
    let port = resolve_port()?;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let app = Router::new().route("/", get(root_handler));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|err| MaccError::Io {
            path: addr.to_string(),
            action: "bind web server".into(),
            source: err,
        })?;
    let local_addr = listener.local_addr().unwrap_or(addr);
    println!("Web server listening on http://{local_addr}");
    axum::serve(listener, app)
        .await
        .map_err(|err| MaccError::Io {
            path: local_addr.to_string(),
            action: "serve web requests".into(),
            source: std::io::Error::new(std::io::ErrorKind::Other, err),
        })?;
    Ok(())
}

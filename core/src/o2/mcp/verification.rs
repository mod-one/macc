//! Apply uses a real isolated lease and the registered stdio command; no model runs.
use super::*;
use crate::tool::{scoped_mcp::RegistrationSpec, ToolSpec};
use rusqlite::OptionalExtension;

pub(crate) fn fingerprint(root: &Path, registration: &RegistrationSpec) -> Result<String> {
    registration.verify(root)?;
    let server = registration.server();
    let command = server["command"]
        .as_str()
        .ok_or_else(|| error("MCP command missing"))?;
    let path = if Path::new(command).is_absolute() {
        Some(std::path::PathBuf::from(command))
    } else {
        std::env::var_os("PATH").and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join(command))
                .find(|p| p.is_file())
        })
    };
    let runtime = path.as_ref().and_then(|p|std::fs::metadata(p).ok()).map(|metadata| {
        json!({"size":metadata.len(),"modified":metadata.modified().ok().and_then(|m|m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d|d.as_nanos().to_string())})
    });
    Ok(digest(
        json!({"registration":registration,"server":server,"path":path,"runtime":runtime})
            .to_string()
            .as_bytes(),
    ))
}

pub fn verify(root: &Path, spec: &ToolSpec) -> Result<()> {
    #[cfg(not(test))]
    let transport = |command: &Value, env: &Value, input: &[u8]| {
        super::probe_process::run(
            root,
            command,
            env,
            input,
            std::time::Duration::from_secs(15),
        )
    };
    // Core unit tests inject an in-process transport. The public CLI integration
    // test exercises the production registered-command subprocess end to end.
    #[cfg(test)]
    let transport = |_: &Value, env: &Value, input: &[u8]| {
        let credentials = Credentials {
            session_id: env["MACC_O2_MCP_SESSION"]
                .as_str()
                .ok_or_else(|| error("Control session missing"))?
                .into(),
            generation: env["MACC_O2_MCP_GENERATION"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| error("Control generation missing"))?,
            token: env["MACC_O2_MCP_TOKEN"]
                .as_str()
                .ok_or_else(|| error("Control capability missing"))?
                .into(),
        };
        let mut output = vec![];
        super::serve(root, credentials, std::io::Cursor::new(input), &mut output)?;
        Ok(output)
    };
    verify_with(root, spec, transport)
}

fn verify_with(
    root: &Path,
    spec: &ToolSpec,
    run: impl FnOnce(&Value, &Value, &[u8]) -> Result<Vec<u8>>,
) -> Result<()> {
    agent::enabled(root)?;
    let registration = spec
        .performer
        .as_ref()
        .and_then(|p| p.scoped_mcp.as_ref())
        .ok_or_else(|| error("Scoped MCP registration unsupported"))?;
    let before = fingerprint(root, registration)?;
    {
        let _guard = store::lock(root)?;
        let (db, _) = database(root)?;
        super::super::schema::audit(
            &db,
            "mcp_verified",
            &spec.id,
            json!({"fingerprint":before,"verified":false}),
        )?;
    }
    // Unique task/role cannot consume a performer transcript or a work item.
    let task = format!("mcp-control-{}", uuid::Uuid::new_v4());
    let identity = crate::coordinator::session_identity::execution_identity(
        root,
        root,
        &task,
        "mcp_control",
        &spec.id,
        &serde_json::to_value(spec).map_err(|e| error(e.to_string()))?,
        &crate::coordinator::session_identity::SessionRouting {
            tier: String::new(),
            mode: "auto".into(),
        },
    )
    .ok_or_else(|| error("MCP control identity could not be created"))?;
    let grant = store::acquire(
        root,
        &store::AcquireRequest {
            identity,
            request_id: uuid::Uuid::new_v4().to_string(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: false,
            checkpoint_id: None,
        },
    )?;
    let (stop, stopped) = std::sync::mpsc::channel();
    let heartbeat_root = root.to_path_buf();
    let heartbeat_key = grant.key.clone();
    let heartbeat = std::thread::spawn(move || -> Result<()> {
        loop {
            match stopped.recv_timeout(std::time::Duration::from_secs(1)) {
                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    store::heartbeat(&heartbeat_root, &heartbeat_key)?
                }
            }
        }
    });
    let result = (|| {
        let env = agent::options(root, &grant.key)?["env"].clone();
        let requests = [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"macc-apply-control","version":crate::version()}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory.search","arguments":{"query":task,"limit":1}}}),
        ];
        let input = requests
            .iter()
            .map(|r| format!("{r}\n"))
            .collect::<String>();
        let bytes = run(&registration.server(), &env, input.as_bytes())?;
        validate(&bytes)?;
        if fingerprint(root, registration)? != before {
            return Err(error(
                "MCP registration/runtime changed during verification",
            ));
        }
        Ok(())
    })();
    // Always revoke before publishing any success, including protocol/launch failures.
    let _ = stop.send(());
    let heartbeat_result = heartbeat
        .join()
        .map_err(|_| error("Control session heartbeat failed"))
        .and_then(|result| result);
    let release = store::release(root, &grant.key, false);
    release.map_err(|e| {
        error(format!(
            "MCP control lease cleanup failed: {e}; inspect session recovery before retrying Apply"
        ))
    })?;
    heartbeat_result?;
    result?;
    let _guard = store::lock(root)?;
    let (db, _) = database(root)?;
    super::super::schema::audit(
        &db,
        "mcp_verified",
        &spec.id,
        json!({"fingerprint":before,"verified":true,"protocol":"2025-11-25","authorized_read":"memory.search","native_client_verified":false}),
    )
}

fn validate(bytes: &[u8]) -> Result<()> {
    let messages = bytes
        .split(|b| *b == b'\n')
        .filter(|b| !b.is_empty())
        .map(store::json::<Value>)
        .collect::<Result<Vec<_>>>()?;
    if messages.len() != 3
        || messages
            .iter()
            .any(|m| m["jsonrpc"] != "2.0" || !m["error"].is_null())
    {
        return Err(error("Invalid MCP verification protocol response"));
    }
    let find = |id| {
        messages
            .iter()
            .find(|m| m["id"] == id)
            .ok_or_else(|| error("Missing MCP verification response ID"))
    };
    let init = find(1)?;
    if init["result"]["serverInfo"]["name"] != "macc-o2"
        || init["result"]["serverInfo"]["version"] != crate::version()
        || init["result"]["protocolVersion"] != "2025-11-25"
    {
        return Err(error(
            "MCP initialization did not negotiate the expected server/protocol",
        ));
    }
    let list = find(2)?;
    let tools = list["result"]["tools"]
        .as_array()
        .ok_or_else(|| error("Invalid MCP tool catalog"))?;
    if !["memory.search", "memory.get"].iter().all(|name| {
        tools
            .iter()
            .any(|t| t["name"] == *name && t["inputSchema"].is_object())
    }) {
        return Err(error("MCP memory read capabilities missing"));
    }
    let read = find(3)?;
    if read["result"]["isError"] != false {
        return Err(error("MCP control read was not authorized"));
    }
    let data: Value = store::json(
        read["result"]["content"][0]["text"]
            .as_str()
            .ok_or_else(|| error("MCP control read content missing"))?
            .as_bytes(),
    )?;
    if !data["records"].is_array() || data["summary_only"] != true || data["schema_version"] != 2 {
        return Err(error("MCP control read returned invalid memory results"));
    }
    Ok(())
}

pub(crate) fn current(root: &Path, tool: &str, registration: &RegistrationSpec) -> Result<bool> {
    let fingerprint = fingerprint(root, registration)?;
    let (db, _) = readonly_database(root)?;
    let raw: Option<String> = sql(db.query_row("SELECT payload_json FROM o2_audit WHERE kind='mcp_verified' AND record_id=?1 ORDER BY rowid DESC LIMIT 1",[tool],|r|r.get(0)).optional())?;
    Ok(raw
        .map(|s| store::json::<Value>(s.as_bytes()))
        .transpose()?
        .is_some_and(|v| v["fingerprint"] == fingerprint && v["verified"] == true))
}

#[cfg(test)]
#[path = "verification_tests.rs"]
mod tests;

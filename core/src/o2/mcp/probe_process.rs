//! Bounded local stdio client. Credentials never enter argv, files or diagnostics.
use super::*;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(super) fn run(
    root: &Path,
    command: &Value,
    env: &Value,
    input: &[u8],
    timeout: Duration,
) -> Result<Vec<u8>> {
    if input.len() > 4096 {
        return Err(error("MCP verification input exceeds 4 KiB"));
    }
    let executable = command["command"]
        .as_str()
        .ok_or_else(|| error("MCP command missing"))?;
    let args = command["args"]
        .as_array()
        .ok_or_else(|| error("MCP arguments missing"))?
        .iter()
        .map(|v| v.as_str().ok_or_else(|| error("Invalid MCP argument")))
        .collect::<Result<Vec<_>>>()?;
    let mut process = Command::new(executable);
    process
        .args(args)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // The registered allowlist is also exercised by the control transport.
    process.env_clear();
    for key in ["PATH", "HOME", "LANG", "TMPDIR"] {
        if let Some(value) = std::env::var_os(key) {
            process.env(key, value);
        }
    }
    for key in crate::tool::scoped_mcp::ENVIRONMENT {
        process.env(
            key,
            env[key]
                .as_str()
                .ok_or_else(|| error("Missing control capability environment"))?,
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }
    let mut child = process.spawn().map_err(|e| {
        error(format!(
            "MCP startup failed for the registered executable: {e}"
        ))
    })?;
    let pid = child.id();
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let read = |stream: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = vec![];
            stream.take(65537).read_to_end(&mut bytes).map(|_| bytes)
        })
    };
    let stdout = read(Box::new(out));
    let stderr = read(Box::new(err));
    let write = child.stdin.take().unwrap().write_all(input);
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if write.is_ok() && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => {
                #[cfg(unix)]
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                break Err(error(if write.is_err() {
                    "MCP initialization input failed"
                } else {
                    "MCP verification timed out"
                }));
            }
        }
    };
    // Descendants must not retain the pipes after the registered server exits.
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    let output = stdout
        .join()
        .map_err(|_| error("MCP output reader failed"))?
        .map_err(|_| error("MCP output read failed"))?;
    let diagnostic = stderr
        .join()
        .map_err(|_| error("MCP diagnostic reader failed"))?
        .map_err(|_| error("MCP diagnostic read failed"))?;
    let status = status?;
    if !status.success() {
        return Err(error(format!("MCP registered server exited with {status}")));
    }
    if output.len() > 65536 || diagnostic.len() > 65536 {
        return Err(error("MCP verification output exceeded 64 KiB"));
    }
    Ok(output)
}

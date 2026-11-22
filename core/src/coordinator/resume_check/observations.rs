//! Collect only the declared input. The verifier owns identity, freshness and result checks.
use super::{error, Source};
use crate::Result;
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Component, Path},
    time::Instant,
};

const MAX_INPUT: usize = 128 * 1024;

pub(super) fn validate(source: &Source) -> Result<()> {
    if let Source::JsonFile { path } | Source::TextFile { path } = source {
        if path.is_empty()
            || path.len() > 512
            || path.contains('\\')
            || path.split('/').any(|part| {
                part.is_empty() || part.starts_with('.') || part.chars().any(char::is_control)
            })
            || !Path::new(path)
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
        {
            return Err(error(
                "Observation file must be a relative project artifact path",
            ));
        }
    }
    Ok(())
}

pub(super) fn collect(
    source: &Source,
    worktree: &Path,
    deadline: Instant,
) -> Result<Option<Value>> {
    validate(source)?;
    match source {
        Source::AdbDevices => {
            adb_devices(SocketAddr::from(([127, 0, 0, 1], 5037)), deadline).map(Some)
        }
        Source::JsonFile { path } | Source::TextFile { path } => {
            crate::project_ownership::validate_path(
                &crate::ProjectPaths::from_root(worktree),
                path,
            )?;
            let Some(bytes) = read_regular(&worktree.join(path), MAX_INPUT)? else {
                return Ok(None);
            };
            match source {
                Source::TextFile { .. } => String::from_utf8(bytes)
                    .map(Value::String)
                    .map(Some)
                    .map_err(|_| error("Observation is not UTF-8")),
                _ => serde_json::from_slice(&bytes)
                    .map(Some)
                    .map_err(|e| error(format!("Invalid observation JSON: {e}"))),
            }
        }
    }
}

/// Open without following a final symlink or waiting on a special file.
pub(super) fn read_regular(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(format!("Cannot open {}: {e}", path.display()))),
    };
    let meta = file.metadata().map_err(error)?;
    if !meta.is_file() || meta.len() > limit as u64 {
        return Err(error(format!(
            "Expected a regular file of at most {limit} bytes: {}",
            path.display()
        )));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() > limit {
        return Err(error(format!(
            "File exceeds {limit} bytes: {}",
            path.display()
        )));
    }
    Ok(Some(bytes))
}

// The ADB host service returns the same serial, transport state and attributes as adb devices -l.
// It never sends a device command, authorizes a device or selects a default serial.
pub(super) fn adb_devices(address: SocketAddr, deadline: Instant) -> Result<Value> {
    let remaining = || {
        deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "ADB observation timed out")
            })
    };
    let query = || -> std::io::Result<Vec<u8>> {
        let mut socket = TcpStream::connect_timeout(&address, remaining()?)?;
        socket.set_write_timeout(Some(remaining()?))?;
        socket.write_all(b"000ehost:devices-l")?;
        let mut read = |buf: &mut [u8]| -> std::io::Result<()> {
            let mut offset = 0;
            while offset < buf.len() {
                socket.set_read_timeout(Some(remaining()?))?;
                let n = socket.read(&mut buf[offset..])?;
                if n == 0 {
                    return Err(std::io::ErrorKind::UnexpectedEof.into());
                }
                offset += n;
            }
            Ok(())
        };
        let mut header = [0; 4];
        read(&mut header)?;
        if &header != b"OKAY" {
            return Err(std::io::Error::other("ADB refused device enumeration"));
        }
        read(&mut header)?;
        let length = std::str::from_utf8(&header)
            .ok()
            .and_then(|s| usize::from_str_radix(s, 16).ok())
            .ok_or_else(|| std::io::Error::other("Invalid ADB reply length"))?;
        let mut bytes = vec![0; length];
        read(&mut bytes)?;
        Ok(bytes)
    };
    let bytes = query().map_err(|e| {
        error(format!(
            "Cannot list the runner's devices. Check the local ADB server: {e}"
        ))
    })?;
    let list = std::str::from_utf8(&bytes).map_err(|_| error("Invalid ADB inventory"))?;
    let mut devices = vec![];
    for line in list.lines().filter(|l| !l.trim().is_empty()) {
        let mut parts = line.split_whitespace();
        let (Some(serial), Some(state)) = (parts.next(), parts.next()) else {
            return Err(error("Incomplete ADB device record"));
        };
        let attributes: std::collections::BTreeMap<_, _> =
            parts.filter_map(|s| s.split_once(':')).collect();
        devices.push(json!({"serial":serial,"state":state,"attributes":attributes}));
    }
    Ok(Value::Array(devices))
}

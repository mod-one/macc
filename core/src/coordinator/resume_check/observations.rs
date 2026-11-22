//! Bounded runner observations for a supervisor-authored verification program.
use super::{adb, error, Source};
use crate::Result;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    net::SocketAddr,
    path::{Component, Path},
    time::Instant,
};

const MAX_INPUT: usize = 128 * 1024;
const MAX_FIELDS: usize = 8;
const MAX_HASH_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn validate(source: &Source) -> Result<()> {
    let path = match source {
        Source::Inputs { fields } => {
            if fields.len() > MAX_FIELDS {
                return Err(error(format!(
                    "At most {MAX_FIELDS} named observations are supported"
                )));
            }
            for (name, field) in fields {
                if name.is_empty()
                    || name.len() > 64
                    || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                {
                    return Err(error(
                        "Observation names must contain 1..64 letters, digits or underscores",
                    ));
                }
                if matches!(field, Source::Inputs { .. }) {
                    return Err(error("Nested observation groups are not supported"));
                }
                validate(field)?;
            }
            return Ok(());
        }
        Source::AdbDevices => return Ok(()),
        Source::JsonFile { path }
        | Source::TextFile { path }
        | Source::PathInfo { path }
        | Source::Sha256File { path } => path,
    };
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
    Ok(())
}

pub(super) enum Reading {
    Observed(Value),
    Missing(String),
}

pub(super) fn collect(source: &Source, worktree: &Path, deadline: Instant) -> Result<Reading> {
    validate(source)?;
    read_source(source, worktree, deadline)
}

fn check_time(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(error("Observation deadline exceeded"));
    }
    Ok(())
}

fn read_source(source: &Source, worktree: &Path, deadline: Instant) -> Result<Reading> {
    check_time(deadline)?;
    let checked_path = |relative: &str| -> Result<_> {
        crate::project_ownership::validate_path(
            &crate::ProjectPaths::from_root(worktree),
            relative,
        )?;
        Ok(worktree.join(relative))
    };
    match source {
        Source::Inputs { fields } => {
            let mut values = Map::new();
            // The object delimiters occupy one byte beyond the per-field accounting.
            let mut size = 1;
            for (name, field) in fields {
                let reading = read_source(field, worktree, deadline)?;
                let Reading::Observed(value) = reading else {
                    return Ok(reading);
                };
                size += name.len() + value.to_string().len() + 4;
                if size > MAX_INPUT {
                    return Err(error("Combined observations exceed the input limit"));
                }
                values.insert(name.clone(), value);
            }
            Ok(Reading::Observed(Value::Object(values)))
        }
        Source::AdbDevices => adb::inventory(SocketAddr::from(([127, 0, 0, 1], 5037)), deadline)
            .map(Reading::Observed),
        Source::PathInfo { path } => {
            let value = match fs::symlink_metadata(checked_path(path)?) {
                Ok(meta) if meta.is_dir() => json!({"kind":"directory"}),
                Ok(meta) if meta.is_file() => json!({"kind":"file","bytes":meta.len()}),
                Ok(_) => {
                    return Err(error(format!(
                        "Expected a regular file or directory at {path}"
                    )))
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({"kind":"missing"}),
                Err(e) => return Err(error(format!("Cannot inspect {path}: {e}"))),
            };
            Ok(Reading::Observed(value))
        }
        Source::Sha256File { path } => {
            let Some(mut file) = open_regular(&checked_path(path)?, MAX_HASH_BYTES)? else {
                return Ok(Reading::Missing(path.clone()));
            };
            let mut digest = Sha256::new();
            let mut buffer = [0; 8192];
            let mut bytes = 0;
            loop {
                check_time(deadline)?;
                let read = file.read(&mut buffer).map_err(error)?;
                if read == 0 {
                    break;
                }
                bytes += read;
                if bytes > MAX_HASH_BYTES {
                    return Err(error("Digest observation exceeds its byte limit"));
                }
                digest.update(&buffer[..read]);
            }
            Ok(Reading::Observed(Value::String(format!(
                "{:x}",
                digest.finalize()
            ))))
        }
        Source::JsonFile { path } | Source::TextFile { path } => {
            let Some(bytes) = read_regular(&checked_path(path)?, MAX_INPUT)? else {
                return Ok(Reading::Missing(path.clone()));
            };
            let value = if matches!(source, Source::TextFile { .. }) {
                Value::String(
                    String::from_utf8(bytes)
                        .map_err(|_| error(format!("Observation {path} is not UTF-8")))?,
                )
            } else {
                serde_json::from_slice(&bytes)
                    .map_err(|e| error(format!("Invalid observation JSON at {path}: {e}")))?
            };
            Ok(Reading::Observed(value))
        }
    }
}

/// Open without following a final symlink or waiting on a special file.
fn open_regular(path: &Path, limit: usize) -> Result<Option<File>> {
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
    Ok(Some(file))
}

pub(super) fn read_regular(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
    let Some(file) = open_regular(path, limit)? else {
        return Ok(None);
    };
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

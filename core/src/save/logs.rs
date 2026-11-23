use super::{redact_secrets_in_text, SaveOptions};
use crate::{MaccError, Result};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::SystemTime,
};

fn io_error(path: &Path, source: io::Error) -> MaccError {
    MaccError::Io {
        path: path.display().to_string(),
        action: "archive logs".into(),
        source,
    }
}

/// Accept files at the log root and descend only into the two approved trees.
/// DirEntry::file_type does not follow symlinks.
fn collect(root: &Path, relative: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let dir = root.join(relative);
    for entry in fs::read_dir(&dir).map_err(|e| io_error(&dir, e))? {
        let entry = entry.map_err(|e| io_error(&dir, e))?;
        let path = relative.join(entry.file_name());
        let kind = entry.file_type().map_err(|e| io_error(&entry.path(), e))?;
        if kind.is_file() {
            files.push(path);
        } else if kind.is_dir()
            && (!relative.as_os_str().is_empty()
                || path == Path::new("coordinator")
                || path == Path::new("performer"))
        {
            collect(root, &path, files)?;
        }
    }
    Ok(())
}

pub(super) fn capture(root: &Path, destination: &Path, opts: &SaveOptions) -> Result<Vec<String>> {
    let mut omitted_by_size = Vec::new();
    match fs::symlink_metadata(root) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(omitted_by_size),
        Err(e) => return Err(io_error(root, e)),
        Ok(meta) if meta.is_symlink() => return Ok(omitted_by_size),
        Ok(_) => {}
    }
    let now = SystemTime::now();
    let max_age = parse_duration_to_seconds(&opts.log_since);
    let mut remaining = parse_size_to_bytes(&opts.log_max_size);
    let mut files = Vec::new();
    collect(root, Path::new(""), &mut files)?;
    files.sort();
    for relative in files {
        let path = root.join(&relative);
        let metadata = fs::symlink_metadata(&path).map_err(|e| io_error(&path, e))?;
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata.modified().map_err(|e| io_error(&path, e))?;
        if now.duration_since(modified).unwrap_or_default().as_secs() > max_age {
            continue;
        }
        let mut bytes = Vec::new();
        if metadata.len() <= remaining {
            // Active logs may grow after stat. Never read past the budget plus one byte.
            fs::File::open(&path)
                .and_then(|file| {
                    file.take(remaining.saturating_add(1))
                        .read_to_end(&mut bytes)
                })
                .map_err(|e| io_error(&path, e))?;
        }
        if metadata.len() > remaining || bytes.len() as u64 > remaining {
            omitted_by_size.push(relative.to_string_lossy().into_owned());
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let size = text.len() as u64;
        let text = if opts.redact_logs {
            redact_secrets_in_text(&text)
        } else {
            text
        };
        let target = destination.join(relative);
        let parent = target.parent().expect("log file has a parent");
        fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        fs::write(&target, text).map_err(|e| io_error(&target, e))?;
        remaining -= size;
    }
    Ok(omitted_by_size)
}

pub fn parse_size_to_bytes(s: &str) -> u64 {
    let s = s.trim().to_ascii_uppercase();
    if s.is_empty() {
        return 50 * 1024 * 1024; // Default fallback
    }

    let (num_part, multiplier) = if s.ends_with("GB") {
        (&s[..s.len() - 2], 1024 * 1024 * 1024)
    } else if s.ends_with("MB") {
        (&s[..s.len() - 2], 1024 * 1024)
    } else if s.ends_with("KB") {
        (&s[..s.len() - 2], 1024)
    } else if s.ends_with('B') {
        (&s[..s.len() - 1], 1)
    } else {
        (s.as_str(), 1)
    };

    match num_part.trim().parse::<u64>() {
        Ok(val) => val.saturating_mul(multiplier),
        Err(_) => 50 * 1024 * 1024, // Fallback
    }
}

pub fn parse_duration_to_seconds(s: &str) -> u64 {
    let s = s.trim().to_ascii_lowercase();
    if s.is_empty() {
        return 7 * 24 * 3600; // Default fallback (7 days)
    }

    let (num_part, multiplier) = if s.ends_with('d') {
        (&s[..s.len() - 1], 24 * 3600)
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], 3600)
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], 60)
    } else if s.ends_with('s') {
        (&s[..s.len() - 1], 1)
    } else {
        (s.as_str(), 1)
    };

    match num_part.trim().parse::<u64>() {
        Ok(val) => val.saturating_mul(multiplier),
        Err(_) => 7 * 24 * 3600, // Fallback
    }
}

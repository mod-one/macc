use crate::{MaccError, ProjectPaths, Result};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct LogFileEntry {
    pub path: PathBuf,
    pub relative: String,
}

pub trait LogsUi {
    fn print_line(&self, line: &str);
}

pub fn select_log_file(
    paths: &ProjectPaths,
    component: &str,
    worktree_filter: Option<&str>,
    task_filter: Option<&str>,
) -> Result<std::path::PathBuf> {
    let normalized = component.to_ascii_lowercase();
    maybe_aggregate_performer_logs(paths, &normalized);
    let mut files = Vec::new();

    if normalized == "all" || normalized == "coordinator" {
        files.extend(collect_log_files(
            &paths.macc_dir.join("log/coordinator"),
            None,
        )?);
    }
    if normalized == "all" || normalized == "performer" {
        files.extend(collect_log_files(
            &paths.macc_dir.join("log/performer"),
            task_filter,
        )?);
        files.extend(collect_performer_worktree_logs(
            &paths.root,
            worktree_filter,
            task_filter,
        )?);
    }

    if files.is_empty() {
        return Err(MaccError::Validation(
            "No logs found. Run `macc coordinator run` or `macc worktree run <id>` first.".into(),
        ));
    }

    files.sort_by(|a, b| {
        let am = std::fs::metadata(a)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let bm = std::fs::metadata(b)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        bm.cmp(&am)
    });
    Ok(files[0].clone())
}

pub fn print_file_tail(path: &std::path::Path, lines: usize, ui: &dyn LogsUi) -> Result<()> {
    let content = std::fs::read_to_string(path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "read log file".into(),
        source: e,
    })?;
    let all = content.lines().collect::<Vec<_>>();
    let start = all.len().saturating_sub(lines);
    for line in &all[start..] {
        ui.print_line(line);
    }
    Ok(())
}

pub fn tail_file_follow(path: &std::path::Path, lines: usize) -> Result<()> {
    let status = std::process::Command::new("tail")
        .arg("-n")
        .arg(lines.to_string())
        .arg("-F")
        .arg(path)
        .status()
        .map_err(|e| MaccError::Io {
            path: "tail".into(),
            action: "follow log file".into(),
            source: e,
        })?;
    if !status.success() {
        return Err(MaccError::Validation(format!(
            "tail failed with status: {}",
            status
        )));
    }
    Ok(())
}

pub fn read_log_content(
    paths: &ProjectPaths,
    component: &str,
    worktree_filter: Option<&str>,
    task_filter: Option<&str>,
) -> Result<String> {
    let path = select_log_file(paths, component, worktree_filter, task_filter)?;
    std::fs::read_to_string(&path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "read log file".into(),
        source: e,
    })
}

/// How much of the coordinator log status and failure analysis read.
pub const STATUS_LOG_TAIL_BYTES: u64 = 256 * 1024;

/// The last `max_bytes` of a log, starting at a full line. Status and
/// failure analysis only look at the latest lines, and a run log grows to
/// megabytes over a long run.
pub fn read_log_tail(
    paths: &ProjectPaths,
    component: &str,
    worktree_filter: Option<&str>,
    task_filter: Option<&str>,
    max_bytes: u64,
) -> Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let path = select_log_file(paths, component, worktree_filter, task_filter)?;
    let io_err = |e: std::io::Error| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "read log file".into(),
        source: e,
    };
    let mut file = std::fs::File::open(&path).map_err(io_err)?;
    let len = file.metadata().map_err(io_err)?.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start)).map_err(io_err)?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    file.read_to_end(&mut bytes).map_err(io_err)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start == 0 {
        return Ok(text);
    }
    // Drop the partial first line.
    Ok(match text.find('\n') {
        Some(newline) => text[newline + 1..].to_string(),
        None => String::new(),
    })
}

fn collect_log_files(
    dir: &std::path::Path,
    task_filter: Option<&str>,
) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    for entry in std::fs::read_dir(dir).map_err(|e| MaccError::Io {
        path: dir.to_string_lossy().into(),
        action: "read log directory".into(),
        source: e,
    })? {
        let path = entry
            .map_err(|e| MaccError::Io {
                path: dir.to_string_lossy().into(),
                action: "iterate log directory".into(),
                source: e,
            })?
            .path();
        if !path.is_file() {
            continue;
        }
        if let Some(filter) = task_filter {
            let name = path
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or_default();
            if !name.contains(filter) {
                continue;
            }
        }
        files.push(path);
    }
    Ok(files)
}

pub fn list_log_entries(paths: &ProjectPaths) -> Result<Vec<LogFileEntry>> {
    maybe_aggregate_performer_logs(paths, "all");
    let log_root = paths.root.join(".macc/log");
    let mut out = Vec::new();
    collect_log_entries(&log_root, &log_root, &mut out)?;
    out.sort_by(|a, b| b.relative.cmp(&a.relative));
    Ok(out)
}

fn maybe_aggregate_performer_logs(paths: &ProjectPaths, normalized_component: &str) {
    if normalized_component == "all" || normalized_component == "performer" {
        if let Err(err) = crate::coordinator::logs::aggregate_performer_logs(&paths.root) {
            tracing::warn!(
                "performer log aggregation failed before log retrieval: {}",
                err
            );
        }
    }
}

pub fn read_log_file(path: &std::path::Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "read log file".into(),
        source: e,
    })
}

fn collect_log_entries(
    dir: &std::path::Path,
    root: &std::path::Path,
    out: &mut Vec<LogFileEntry>,
) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(iter) => iter,
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let entry = entry.map_err(|e| MaccError::Io {
            path: dir.to_string_lossy().into(),
            action: "iterate log directory".into(),
            source: e,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_log_entries(&path, root, out)?;
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "md" && ext != "log" && ext != "txt" {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .ok()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| path.display().to_string());
        out.push(LogFileEntry { path, relative });
    }
    Ok(())
}

fn collect_performer_worktree_logs(
    root: &std::path::Path,
    worktree_filter: Option<&str>,
    task_filter: Option<&str>,
) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    let base = root.join(".macc/worktree");
    if !base.exists() {
        return Ok(files);
    }
    for entry in std::fs::read_dir(&base).map_err(|e| MaccError::Io {
        path: base.to_string_lossy().into(),
        action: "read worktree log base".into(),
        source: e,
    })? {
        let wt = entry
            .map_err(|e| MaccError::Io {
                path: base.to_string_lossy().into(),
                action: "iterate worktree log base".into(),
                source: e,
            })?
            .path();
        if !wt.is_dir() {
            continue;
        }
        if let Some(filter) = worktree_filter {
            let needle = filter.to_ascii_lowercase();
            let text = wt.display().to_string().to_ascii_lowercase();
            if !text.contains(&needle) {
                continue;
            }
        }
        let log_dir = wt.join(".macc/log/performer");
        files.extend(collect_log_files(&log_dir, task_filter)?);
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_log_tail_starts_at_a_full_line() {
        let root = std::env::temp_dir().join(format!(
            "macc_log_tail_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let paths = ProjectPaths::from_root(&root);
        let dir = paths.macc_dir.join("log/coordinator");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("run-1.md"), "first line\nsecond line\nlast line\n").unwrap();

        let tail = read_log_tail(&paths, "coordinator", None, None, 15).unwrap();
        assert_eq!(tail, "last line\n");
        let whole = read_log_tail(&paths, "coordinator", None, None, 1024).unwrap();
        assert_eq!(whole, "first line\nsecond line\nlast line\n");
        let _ = std::fs::remove_dir_all(root);
    }
}

use crate::{MaccError, Result};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

pub fn aggregate_performer_logs(repo_root: &Path) -> Result<usize> {
    aggregate_performer_logs_sync(repo_root)
}

pub async fn aggregate_performer_logs_async(repo_root: &Path) -> Result<usize> {
    aggregate_performer_logs_sync(repo_root)
}

fn aggregate_performer_logs_sync(repo_root: &Path) -> Result<usize> {
    let worktrees_root = repo_root.join(".macc").join("worktree");
    let target_root = repo_root.join(".macc").join("log").join("performer");
    fs::create_dir_all(&target_root).map_err(|e| MaccError::Io {
        path: target_root.to_string_lossy().into(),
        action: "create performer log aggregation directory".into(),
        source: e,
    })?;

    let mut entries = Vec::new();
    if !worktrees_root.exists() {
        write_index(&target_root.join("index.json"), &entries)?;
        return Ok(0);
    }

    let mut copied = 0usize;
    let worktree_entries = fs::read_dir(&worktrees_root).map_err(|e| MaccError::Io {
        path: worktrees_root.to_string_lossy().into(),
        action: "read worktree root for performer logs".into(),
        source: e,
    })?;
    for wt in worktree_entries {
        let wt = wt.map_err(|e| MaccError::Io {
            path: worktrees_root.to_string_lossy().into(),
            action: "iterate worktree root for performer logs".into(),
            source: e,
        })?;
        let wt_path = wt.path();
        if !wt
            .file_type()
            .map_err(|e| MaccError::Io {
                path: wt_path.to_string_lossy().into(),
                action: "read worktree entry type".into(),
                source: e,
            })?
            .is_dir()
        {
            continue;
        }
        let wt_name = wt_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("worktree");
        let performer_dir = wt_path.join(".macc").join("log").join("performer");
        if !performer_dir.is_dir() {
            continue;
        }

        let log_entries = fs::read_dir(&performer_dir).map_err(|e| MaccError::Io {
            path: performer_dir.to_string_lossy().into(),
            action: "read performer worktree log dir".into(),
            source: e,
        })?;
        for file in log_entries {
            let file = file.map_err(|e| MaccError::Io {
                path: performer_dir.to_string_lossy().into(),
                action: "iterate performer worktree log dir".into(),
                source: e,
            })?;
            let source = file.path();
            if !is_markdown_file(&source) {
                continue;
            }

            let source_name = source
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("task.md");
            let target_name = format!("{}--{}", sanitize_name(wt_name), sanitize_name(source_name));
            let target = target_root.join(target_name);

            copy_if_changed(&source, &target)?;
            copied += 1;

            let metadata = fs::metadata(&source).map_err(|e| MaccError::Io {
                path: source.to_string_lossy().into(),
                action: "read performer source metadata".into(),
                source: e,
            })?;
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            entries.push(json!({
                "worktree": wt_name,
                "source": source.to_string_lossy().to_string(),
                "target": target.to_string_lossy().to_string(),
                "bytes": metadata.len(),
                "mtime_epoch": mtime,
            }));
        }
    }

    entries.sort_by(|a, b| {
        let ka = a
            .get("target")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let kb = b
            .get("target")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        ka.cmp(&kb)
    });

    write_index(&target_root.join("index.json"), &entries)?;
    Ok(copied)
}

fn write_index(path: &Path, entries: &[serde_json::Value]) -> Result<()> {
    let generated_at_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let index = json!({
        "schema_version": 1,
        "generated_at_epoch": generated_at_epoch,
        "entries": entries,
    });
    let body = serde_json::to_vec_pretty(&index).map_err(|e| {
        MaccError::Validation(format!(
            "serialize performer log index '{}': {}",
            path.display(),
            e
        ))
    })?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, body).map_err(|e| MaccError::Io {
        path: tmp.to_string_lossy().into(),
        action: "write performer log index temp file".into(),
        source: e,
    })?;
    fs::rename(&tmp, path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "persist performer log index".into(),
        source: e,
    })?;
    Ok(())
}

fn copy_if_changed(src: &PathBuf, dst: &PathBuf) -> Result<()> {
    let src_bytes = fs::read(src).map_err(|e| MaccError::Io {
        path: src.to_string_lossy().into(),
        action: "read performer source log".into(),
        source: e,
    })?;
    if let Ok(existing) = fs::read(dst) {
        if existing == src_bytes {
            return Ok(());
        }
    }
    fs::write(dst, src_bytes).map_err(|e| MaccError::Io {
        path: dst.to_string_lossy().into(),
        action: "write aggregated performer log".into(),
        source: e,
    })?;
    Ok(())
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

fn sanitize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    if out.is_empty() {
        "log".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn aggregates_worktree_performer_markdown_logs_into_root_log_dir() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let repo_root = std::env::temp_dir().join(format!("macc_performer_logs_test_{}", nonce));
        let _ = fs::remove_dir_all(&repo_root);
        let worktree_log_dir = repo_root.join(".macc/worktree/worker-01/.macc/log/performer");
        fs::create_dir_all(&worktree_log_dir).expect("create worktree performer log dir");
        let source = worktree_log_dir.join("TASK-001-.md");
        fs::write(&source, "# performer log\nok\n").expect("write source performer log");

        let copied = aggregate_performer_logs(&repo_root).expect("aggregate performer logs");
        assert_eq!(copied, 1);

        let aggregated = repo_root.join(".macc/log/performer/worker-01--TASK-001-.md");
        let index = repo_root.join(".macc/log/performer/index.json");
        assert!(aggregated.exists(), "aggregated log should exist");
        assert!(index.exists(), "performer index should exist");
        assert_eq!(
            fs::read_to_string(&aggregated).expect("read aggregated log"),
            "# performer log\nok\n"
        );
        fs::remove_dir_all(&repo_root).expect("cleanup temp repo");
    }
}

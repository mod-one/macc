//! Canonical source fingerprint shared by all code providers.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Serialize, Deserialize)]
pub struct SourceSnapshot {
    pub version: u32,
    pub project_root: String,
    pub revision: String,
    pub files: BTreeMap<String, String>,
    pub digest: String,
    pub excluded: usize,
    pub truncated: bool,
}
pub fn source_snapshot(
    root: &Path,
    p: &super::super::policy::CodePolicy,
) -> Result<SourceSnapshot> {
    let mut files = BTreeMap::new();
    let mut excluded = 0;
    let names = crate::git::tracked_source_files(root)?;
    let truncated = names.len() > p.max_files as usize;
    for name in names.into_iter().take(p.max_files as usize) {
        match paths::source(root, &name, p.max_file_bytes) {
            Ok((_, data)) => {
                if std::str::from_utf8(&data).is_ok()
                    && paths::safe_text(std::str::from_utf8(&data).unwrap()).is_ok()
                {
                    files.insert(name, digest(&data));
                } else {
                    excluded += 1;
                }
            }
            Err(_) => excluded += 1,
        }
    }
    let digest = digest(
        serde_json::to_string(&files)
            .map_err(|e| error(e.to_string()))?
            .as_bytes(),
    );
    Ok(SourceSnapshot {
        version: 1,
        project_root: root
            .canonicalize()
            .map_err(|e| error(e.to_string()))?
            .display()
            .to_string(),
        revision: crate::git::head_commit(root)?,
        files,
        digest,
        excluded,
        truncated,
    })
}

pub(super) fn ignored_paths(
    root: &Path,
    names: &[String],
    custom_name: Option<&str>,
) -> Result<std::collections::BTreeSet<String>> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let custom = custom_name
        .map(|name| root.join(name))
        .filter(|p| p.exists());
    if let Some(name) = custom_name.filter(|_| custom.is_some()) {
        paths::source(root, name, 1048576)?;
    }
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root);
    if let Some(custom) = custom {
        cmd.arg("-c")
            .arg(format!("core.excludesFile={}", custom.display()));
    }
    let mut child = cmd
        .args(["check-ignore", "--no-index", "-z", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| error(e.to_string()))?;
    let input = names.join("\0") + "\0";
    // Write concurrently: a large ignore result can fill the stdout pipe.
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| error("Missing git stdin"))?;
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = child.wait_with_output().map_err(|e| error(e.to_string()))?;
    writer
        .join()
        .map_err(|_| error("Git ignore writer failed"))?
        .map_err(|e| error(e.to_string()))?;
    if !output.status.success() && output.status.code() != Some(1) {
        return Err(error("Git ignore validation failed"));
    }
    Ok(String::from_utf8(output.stdout)
        .map_err(|e| error(e.to_string()))?
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

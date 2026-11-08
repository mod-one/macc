//! Ask Git itself about user policy, excluding MACC blocks in an isolated mirror.
use super::*;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

pub(super) fn git(root: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(root);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
    ] {
        command.env_remove(key);
    }
    command
}

pub(super) fn covered(
    root: &Path,
    user_exclude: &str,
    candidates: &[(String, bool)],
) -> Result<BTreeSet<(String, bool)>> {
    if candidates.is_empty() {
        return Ok(BTreeSet::new());
    }
    let mirror = tempfile::tempdir().map_err(crate::persistent::error)?;
    let init = git(mirror.path())
        .args(["init", "-q", "--template="])
        .output()
        .map_err(crate::persistent::error)?;
    if !init.status.success() {
        return Err(crate::persistent::error(
            "Cannot create Git ignore-policy mirror",
        ));
    }
    fs::create_dir_all(mirror.path().join(".git/info")).map_err(crate::persistent::error)?;
    fs::write(mirror.path().join(".git/info/exclude"), user_exclude)
        .map_err(crate::persistent::error)?;
    let mut ignore_dirs = BTreeSet::from([PathBuf::new()]);
    for (relative, directory) in candidates {
        let target = mirror.path().join(relative);
        fs::create_dir_all(if *directory {
            &target
        } else {
            target.parent().unwrap()
        })
        .map_err(crate::persistent::error)?;
        let mut parent = Path::new(relative).parent();
        while let Some(dir) = parent {
            ignore_dirs.insert(dir.to_path_buf());
            parent = dir.parent();
        }
    }
    for dir in ignore_dirs {
        let source = root.join(&dir).join(".gitignore");
        if source
            .symlink_metadata()
            .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
        {
            let mut text = fs::read_to_string(source).map_err(crate::persistent::error)?;
            if dir.as_os_str().is_empty() {
                // Retain today's portable boundary, never historical per-tool extras.
                text = crate::gitignore_policy::rewrite(&text, &BTreeSet::new());
            }
            fs::write(mirror.path().join(dir).join(".gitignore"), text)
                .map_err(crate::persistent::error)?;
        }
    }
    let mut command = git(mirror.path());
    let case = git(root)
        .args(["config", "--get", "core.ignoreCase"])
        .output()
        .map_err(crate::persistent::error)?;
    if case.status.success() {
        command.arg("-c").arg(format!(
            "core.ignoreCase={}",
            String::from_utf8_lossy(&case.stdout).trim()
        ));
    }
    let excludes = git(root)
        .args(["config", "--path", "--get", "core.excludesFile"])
        .output()
        .map_err(crate::persistent::error)?;
    if excludes.status.success() {
        let path = PathBuf::from(
            String::from_utf8(excludes.stdout)
                .map_err(crate::persistent::error)?
                .trim(),
        );
        let absolute = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        command
            .arg("-c")
            .arg(format!("core.excludesFile={}", absolute.display()));
    }
    let mut child = command
        .args(["check-ignore", "--no-index", "-z", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(crate::persistent::error)?;
    let input: Vec<u8> = candidates
        .iter()
        .flat_map(|(p, dir)| {
            let mut path = p.as_bytes().to_vec();
            if *dir {
                path.push(b'/');
            }
            path.push(0);
            path
        })
        .collect();
    // Feed and drain concurrently: large inventories must not deadlock on pipe capacity.
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output().map_err(crate::persistent::error)?;
    writer
        .join()
        .map_err(|_| crate::persistent::error("Git ignore-policy writer failed"))?
        .map_err(crate::persistent::error)?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err(crate::persistent::error(format!(
            "Git ignore-policy check failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let ignored: BTreeSet<_> = output
        .stdout
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| p.to_vec())
        .collect();
    Ok(candidates
        .iter()
        .filter(|(p, dir)| {
            let query = format!("{p}{}", if *dir { "/" } else { "" });
            ignored.contains(query.as_bytes())
        })
        .cloned()
        .collect())
}

//! Exercise restore through argument parsing and real command dispatch.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn files(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            result.extend(files(&path));
        } else {
            result.insert(path.clone(), fs::read(path).unwrap());
        }
    }
    result
}

#[test]
fn preview_preserves_project_storage_and_git_exclusions() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let root = project.path();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_macc"))
            .current_dir(root)
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .args(args)
            .output()
            .unwrap()
    };
    assert!(Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status()
        .unwrap()
        .success());
    fs::create_dir_all(root.join(".macc/state")).unwrap();
    let config = root.join(".macc/macc.yaml");
    fs::write(&config, "tools:\n  enabled: []\n").unwrap();
    fs::write(root.join(".macc/state/tool-sessions.json"), "{}").unwrap();
    let saved = run(&["save", "snapshot", "--offline"]);
    assert!(
        saved.status.success(),
        "{}",
        String::from_utf8_lossy(&saved.stderr)
    );
    fs::write(config, "# local edits\ntools:\n  enabled: []\n").unwrap();
    fs::write(root.join(".git/info/exclude"), "*.local\n").unwrap();
    // Saving sessions may leave an advisory lock. A preview must not create it.
    let lock = root.join(".macc/state/tool-sessions.json.lock");
    if lock.exists() {
        fs::remove_file(lock).unwrap();
    }
    let before = files(root);
    let rejected = run(&[
        "restore",
        "snapshot",
        "--dry-run",
        "--apply",
        "--yes",
        "--offline",
    ]);
    let diagnostic = String::from_utf8_lossy(&rejected.stderr);
    assert_eq!(rejected.status.code(), Some(2), "{diagnostic}");
    assert!(diagnostic.contains("--apply") && diagnostic.contains("--dry-run"));
    assert_eq!(files(root), before);
    let preview = run(&[
        "restore",
        "snapshot",
        "--dry-run",
        "--sessions",
        "--offline",
    ]);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(String::from_utf8_lossy(&preview.stdout).contains("Restore preview"));
    assert_eq!(files(root), before);
}

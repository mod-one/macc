//! Same checkout name and two distinct, case-sensitive Git origins.
#[cfg(target_os = "linux")]
#[test]
fn restore_rejects_a_case_distinct_origin_before_writing_config() {
    use std::{fs, path::Path, process::Command};

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let source = temp.path().join("one/project");
    let target = temp.path().join("two/project");
    let config = |parallel| {
        format!(
            "tools:\n  enabled: []\nautomation:\n  coordinator:\n    max_parallel: {parallel}\n"
        )
    };
    for (root, origin, parallel) in [(&source, "Systems.git", 2), (&target, "systems.git", 9)] {
        fs::create_dir_all(root.join(".macc")).unwrap();
        let remote = temp.path().join(origin);
        assert!(Command::new("git")
            .args(["init", "--bare", "-q"])
            .arg(&remote)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .current_dir(root)
            .args(["remote", "add", "origin"])
            .arg(remote)
            .status()
            .unwrap()
            .success());
        fs::write(root.join(".macc/macc.yaml"), config(parallel)).unwrap();
    }
    let run = |root: &Path, args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_macc"))
            .current_dir(root)
            .args(args)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .output()
            .unwrap()
    };
    let save = run(
        &source,
        &["save", "settings", "--only", "config", "--offline"],
    );
    assert!(
        save.status.success(),
        "{}",
        String::from_utf8_lossy(&save.stderr)
    );
    let restore = run(&target, &["restore", "settings", "--offline"]);
    assert!(!restore.status.success());
    assert!(String::from_utf8_lossy(&restore.stderr).contains("MACC-RESTORE-2002"));
    assert_eq!(
        fs::read_to_string(target.join(".macc/macc.yaml")).unwrap(),
        config(9)
    );

    assert!(Command::new("git")
        .current_dir(&target)
        .args(["remote", "set-url", "origin"])
        .arg(temp.path().join("Systems.git"))
        .status()
        .unwrap()
        .success());
    let restore = run(&target, &["restore", "settings", "--offline"]);
    assert!(
        restore.status.success(),
        "{}",
        String::from_utf8_lossy(&restore.stderr)
    );
    assert_eq!(
        fs::read_to_string(target.join(".macc/macc.yaml")).unwrap(),
        config(2)
    );
}

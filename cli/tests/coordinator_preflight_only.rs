use std::{fs, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap()
        .success());
}

#[test]
fn preflight_only_is_terminal_before_coordinator_setup() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    let macc = root.join(".macc");
    fs::create_dir(&macc).unwrap();
    fs::write(macc.join("macc.yaml"), "tools:\n  enabled: []\n").unwrap();
    git(root, &["init", "-qb", "main"]);
    git(root, &["add", ".macc/macc.yaml"]);
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "base",
        ],
    );

    for action in ["run", "dispatch", "control-plane-run"] {
        for (branch, allow_create) in [("main", false), ("missing", false), ("missing", true)] {
            let mut cli = Command::new(env!("CARGO_BIN_EXE_macc"));
            cli.current_dir(root)
                .args(["coordinator", "--preflight-only"]);
            cli.args(["--reference-branch", branch]);
            if allow_create {
                cli.arg("--create-reference-branch");
            }
            let output = cli.arg(action).output().unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(
                output.status.success(),
                branch == "main" || allow_create,
                "{action}: {stdout} {stderr}"
            );
            if branch == "missing" && !allow_create {
                assert!(stderr.contains("Preflight failed"), "{stderr}");
            } else if branch == "main" {
                assert!(stdout.contains("Preflight: OK"), "{stdout}");
            }
            assert!(!root.join(".git/refs/heads/missing").exists());
            assert!(!stdout.contains("Coordinator log file"));
            let entries: Vec<_> = fs::read_dir(&macc)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            assert_eq!(entries, ["macc.yaml"], "{action} wrote runtime files");
        }
    }
}

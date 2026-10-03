//! Launch questions are exercised with an actual controlling terminal for every client.
#![cfg(unix)]
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;
fn project() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    std::fs::create_dir_all(r.join(".macc")).unwrap();
    std::fs::write(
        r.join(".macc/macc.yaml"),
        "tools:\n  enabled: [codex]\nautomation:\n  coordinator:\n    reference_branch: master\n",
    )
    .unwrap();
    std::fs::write(r.join("prd.json"), "{\"tasks\":[]}").unwrap();
    std::fs::write(r.join(".gitignore"),".macc/\nperformer.sh\nworktree.prd.json\n.antigravitycli/\n.agents/\nGEMINI.md\n.claude/\nCLAUDE.md\n.codex/\nAGENTS.md\n.gemini/\n.geminiignore\n.vibe/\n").unwrap();
    for args in [
        vec!["init", "-q", "-b", "master"],
        vec!["config", "user.name", "Launch Test"],
        vec!["config", "user.email", "launch@example.invalid"],
        vec!["add", "."],
        vec!["commit", "-qm", "Initialize fixture"],
    ] {
        assert!(Command::new("git")
            .arg("-C")
            .arg(r)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
    t
}
#[test]
fn all_three_clients_offer_supervision_before_launch() {
    for client in ["tui", "web", "none"] {
        let temp = project();
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_macc"));
        cmd.args([
            "--cwd",
            temp.path().to_str().unwrap(),
            "coordinator",
            "run",
            "--client",
            client,
        ]);
        cmd.env("MACC_INTERNAL_INVOCATION", "0");
        let mut child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut text = String::new();
            let mut buffer = [0; 1024];
            while let Ok(n) = reader.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                text.push_str(&String::from_utf8_lossy(&buffer[..n]));
                if text.contains("Also start the supervisor") {
                    break;
                }
            }
            let _ = tx.send(text);
        });
        let output = rx.recv_timeout(Duration::from_secs(10));
        let _ = child.kill();
        let _ = child.wait();
        drop(pair.master);
        let _ = thread.join();
        let output = output.expect("launch did not ask for supervisor");
        assert!(output.contains("[Y/n]"), "client {client}: {output}");
        assert!(
            !temp
                .path()
                .join(".macc/state/coordinator-supervisor.json")
                .exists(),
            "nothing starts before the answer"
        );
    }
}
#[test]
fn explicit_supervisor_and_opt_out_conflict() {
    let output = Command::new(env!("CARGO_BIN_EXE_macc"))
        .args(["coordinator", "run", "--supervisor", "--no-supervisor"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));
}

#[test]
fn unattended_run_starts_a_detached_supervisor_and_stop_shuts_it_down() {
    detached_lifecycle(false);
}
#[test]
fn terminal_hangup_preserves_coordinator_and_supervisor() {
    detached_lifecycle(true);
}
fn detached_lifecycle(terminal: bool) {
    use macc_core::coordinator_storage::{
        CoordinatorSnapshot, CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage,
    };
    use macc_core::ProjectPaths;
    let temp = project();
    let root = temp.path();
    let prd = serde_json::json!({"tasks":[
        {"id":"SUBJECT","title":"already delivered","priority":"1","state":"merged"},
        {"id":"APPROVAL","title":"external approval","priority":"1","state":"waiting_approval","dependencies":["SUBJECT"],"gate":{"kind":"human_approval","subject_task":"SUBJECT","required_approvers":[{"role":"OWNER"}],"quorum":"all"}}
    ]});
    std::fs::write(root.join("prd.json"), prd.to_string()).unwrap();
    let paths = ProjectPaths::from_root(root);
    let mut snapshot = CoordinatorSnapshot::empty();
    snapshot.registry.tasks = serde_json::from_value(prd["tasks"].clone()).unwrap();
    SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths))
        .save_snapshot(&snapshot)
        .unwrap();
    assert!(Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["add", "prd.json"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["commit", "-qm", "Add human gate fixture"])
        .status()
        .unwrap()
        .success());
    struct Stop(std::path::PathBuf);
    impl Drop for Stop {
        fn drop(&mut self) {
            let _ = Command::new(env!("CARGO_BIN_EXE_macc"))
                .args(["--cwd", self.0.to_str().unwrap(), "coordinator", "stop"])
                .env("MACC_INTERNAL_INVOCATION", "1")
                .output();
        }
    }
    let _cleanup = Stop(root.into());
    if terminal {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_macc"));
        cmd.args([
            "--cwd",
            root.to_str().unwrap(),
            "coordinator",
            "run",
            "--no-client",
        ]);
        cmd.env("MACC_INTERNAL_INVOCATION", "0");
        let mut child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let mut writer = pair.master.take_writer().unwrap();
        writer.write_all(b"y\n").unwrap();
        drop(writer);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (tx, rx) = mpsc::channel();
        let reader_thread = std::thread::spawn(move || {
            let mut output = String::new();
            let mut buf = [0; 1024];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                output.push_str(&String::from_utf8_lossy(&buf[..n]));
                if output.contains("Supervisor attached") {
                    break;
                }
            }
            let _ = tx.send(output);
        });
        let response = rx.recv_timeout(Duration::from_secs(20));
        drop(pair.master); // Close the controlling terminal, delivering SIGHUP.
        if response.is_err() {
            let _ = child.kill();
        }
        let _ = child.wait();
        let _ = reader_thread.join();
        assert!(response
            .expect("detached startup timed out")
            .contains("Supervisor attached"));
    } else {
        let output = Command::new(env!("CARGO_BIN_EXE_macc"))
            .args([
                "--cwd",
                root.to_str().unwrap(),
                "coordinator",
                "run",
                "--no-client",
            ])
            .env("MACC_INTERNAL_INVOCATION", "0")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("Supervisor attached"));
    }
    let pid: i32 = std::fs::read_to_string(root.join(".macc/state/supervisor.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let coordinator: i32 = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(root.join(".macc/state/coordinator-supervisor.json")).unwrap(),
    )
    .unwrap()["coordinator_pid"]
        .as_i64()
        .unwrap() as i32;
    assert_eq!(unsafe { libc::getsid(pid) }, pid);
    assert_eq!(unsafe { libc::getsid(coordinator) }, coordinator);
    std::thread::sleep(Duration::from_millis(200));
    assert!(macc_core::coordinator::helpers::is_pid_running(pid as i64));
    assert!(macc_core::coordinator::helpers::is_pid_running(
        coordinator as i64
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_macc"))
        .args(["--cwd", root.to_str().unwrap(), "coordinator", "stop"])
        .env("MACC_INTERNAL_INVOCATION", "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!root.join(".macc/state/supervisor.pid").exists());
}

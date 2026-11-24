//! Stop through the public CLI with a private session owned by this fixture.
#![cfg(target_os = "linux")]
use macc_core::{
    coordinator::managed_command_registry::{ManagedCommandRecord, ManagedCommandRegistry},
    process_ownership::launched_group::{LaunchedGroup, Scope},
    ProjectPaths,
};
use std::{
    fs,
    os::unix::process::CommandExt,
    path::Path,
    process::{Child, Command},
    time::Duration,
};

struct FixtureChild(Child);
impl Drop for FixtureChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn stop(root: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_macc"))
        .current_dir(root)
        .args(["coordinator", "stop"])
        .output()
        .unwrap()
}

#[test]
fn stop_requires_the_registered_generation_and_counts_only_delivered_signals() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join(".macc")).unwrap();
    fs::write(root.join(".macc/macc.yaml"), "tools:\n  enabled: []\n").unwrap();
    let paths = ProjectPaths::from_root(root);
    let mut command = Command::new("sleep");
    command.current_dir(root).arg0("coordinator.sh").arg("120");
    let generation = "28a56d53-c17c-45ce-b955-31cd519bf408".parse().unwrap();
    let scope = Scope::CoordinatorSession;
    command.env(scope.env(), format!("{generation}"));
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = FixtureChild(command.spawn().unwrap());
    let leader = child.0.id() as i32;
    let registry = ManagedCommandRegistry::new(root);
    let mut record = ManagedCommandRecord::new(&paths, "run", leader);
    let assert_alive = |child: &mut Child| assert!(child.try_wait().unwrap().is_none());

    // Same name, repository and process-group layout as a real coordinator.
    assert!(stop(root).status.success());
    assert_alive(&mut child.0);

    registry.upsert(record.clone()).unwrap();
    let legacy = stop(root);
    assert!(!legacy.status.success());
    assert!(String::from_utf8_lossy(&legacy.stderr).contains("no verifiable launch identity"));
    assert_alive(&mut child.0);

    // A valid PID/PGID now belongs to a different launch generation.
    record.group = Some(LaunchedGroup {
        leader,
        scope,
        leader_identity: Some("previous generation".into()),
        generation: "f7438ddc-dce8-49ea-b138-c4e32bc430ea".parse().unwrap(),
    });
    registry.upsert(record.clone()).unwrap();
    let stale = stop(root);
    assert!(stale.status.success(), "{stale:?}");
    assert!(String::from_utf8_lossy(&stale.stdout).contains("Signaled 0 verified processes."));
    assert_alive(&mut child.0);

    let group = LaunchedGroup::from_child(leader, generation, scope);
    record.group = Some(group.clone());
    registry.upsert(record).unwrap();
    let sent = stop(root);
    assert!(sent.status.success(), "{sent:?}");
    assert!(String::from_utf8_lossy(&sent.stdout).contains("Signaled 1 verified processes."));
    assert!(!child.0.wait().unwrap().success());
    assert_eq!(group.stop(false, Duration::ZERO).unwrap(), 0);
}

#[test]
fn force_stop_leaves_an_unmarked_member_of_the_worker_group_running() {
    let scope = Scope::PerformerGroup;
    let generation = "52cc45b4-c4b2-4720-a808-98c3d5632108".parse().unwrap();
    let mut command = Command::new("sleep");
    command
        .arg("120")
        .process_group(0)
        .env(scope.env(), format!("{generation}"));
    // Exercise escalation against this fixture child only.
    unsafe {
        command.pre_exec(|| {
            if libc::signal(libc::SIGTERM, libc::SIG_IGN) == libc::SIG_ERR {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut worker = FixtureChild(command.spawn().unwrap());
    let leader = worker.0.id() as i32;
    let mut other = FixtureChild(
        Command::new("sleep")
            .arg("120")
            .process_group(leader)
            .env_remove(scope.env())
            .spawn()
            .unwrap(),
    );
    let group = LaunchedGroup::from_child(leader, generation, scope);
    assert_eq!(group.stop(false, Duration::ZERO).unwrap(), 1);
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(worker.0.wait().unwrap().signal(), Some(libc::SIGKILL));
    assert!(other.0.try_wait().unwrap().is_none());
    assert_eq!(group.stop(false, Duration::ZERO).unwrap(), 0);
}

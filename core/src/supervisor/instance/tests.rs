use super::*;
use std::process::{Child, Command};

struct FixtureProcess(Child);
impl FixtureProcess {
    fn start() -> Self {
        Self(Command::new("sleep").arg("30").spawn().unwrap())
    }
    fn start_locked(root: &Path) -> Self {
        use std::os::unix::process::CommandExt;
        let mut cmd = Command::new("sleep");
        cmd.arg("30")
            .stdin(fs::File::create(root.join(LOCK)).unwrap());
        // SAFETY: flock is a syscall on the child’s already-installed stdin.
        unsafe {
            cmd.pre_exec(|| {
                if libc::flock(0, libc::LOCK_EX | libc::LOCK_NB) == 0 {
                    Ok(())
                } else {
                    Err(io::Error::last_os_error())
                }
            });
        }
        Self(cmd.spawn().unwrap())
    }
    fn register(&self, root: &Path) -> Instance {
        let pid = self.0.id();
        let instance = Instance {
            pid,
            birth: crate::process_ownership::process_identity(pid.into()).unwrap(),
            generation: Uuid::new_v4(),
            project: root.canonicalize().unwrap(),
        };
        crate::atomic_write(
            &ProjectPaths::from_root(root),
            &root.join(RECORD),
            &serde_json::to_vec(&instance).unwrap(),
        )
        .unwrap();
        fs::write(root.join(PID), pid.to_string()).unwrap();
        instance
    }
}
impl Drop for FixtureProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn legacy_pid_and_reused_native_pid_cannot_signal_a_foreign_process() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join(".macc/state")).unwrap();
    let mut foreign = FixtureProcess::start();
    fs::write(root.join(PID), foreign.0.id().to_string()).unwrap();
    assert!(!stop(root, None).unwrap());
    assert!(!root.join(PID).exists());

    let _lock = lock_if_idle(root).unwrap().unwrap();
    let mut old = foreign.register(root);
    old.birth = "previous occupant".into();
    fs::write(root.join(RECORD), serde_json::to_vec(&old).unwrap()).unwrap();
    assert!(stop(root, None)
        .unwrap_err()
        .to_string()
        .contains("identity"));
    assert!(!old.stop_requested());
    assert!(old.matches(root).unwrap());
    assert!(foreign.0.try_wait().unwrap().is_none());
}

#[test]
fn replacement_during_stop_keeps_its_process_and_registration() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join(".macc/state")).unwrap();
    let _lock = lock_if_idle(root).unwrap().unwrap();
    let first = FixtureProcess::start();
    let old = first.register(root);
    let mut next_process = FixtureProcess::start();
    std::thread::scope(|scope| {
        let stopping = scope.spawn(|| stop(root, Some(old.generation)));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !old.stop_requested() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        // The first owner has received the request. Publish a successor before
        // the stopping client can take the lifetime lock for cleanup.
        let next = next_process.register(root);
        assert!(stopping.join().unwrap().unwrap());
        old.clear(root).unwrap();
        assert!(next.matches(root).unwrap());
        assert!(!next.stop_requested());
        assert!(!stop(root, Some(old.generation)).unwrap());
        assert!(next_process.0.try_wait().unwrap().is_none());
    });
}

#[test]
fn verified_owner_receives_term_but_old_handle_cannot_escalate_after_replacement() {
    use std::os::unix::process::ExitStatusExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join(".macc/state")).unwrap();
    let mut process = FixtureProcess::start_locked(root);
    let old = process.register(root);
    let next = process.register(root);
    if let Some(handle) = signal::Handle::open(process.0.id()).unwrap() {
        handle.send(root, &old, libc::SIGKILL).unwrap();
        assert!(process.0.try_wait().unwrap().is_none());
        assert!(stop(root, Some(next.generation)).unwrap());
        assert_eq!(process.0.wait().unwrap().signal(), Some(libc::SIGTERM));
        assert!(!root.join(PID).exists());
    }
}

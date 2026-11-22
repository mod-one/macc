//! Verify failure reporting and preservation of existing data through the public CLI.
use std::{fs, path::Path, process::Command};

struct Fixture {
    project: tempfile::TempDir,
    home: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let f = Self {
            project: tempfile::tempdir().unwrap(),
            home: tempfile::tempdir().unwrap(),
        };
        let root = f.project.path();
        assert!(Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success());
        fs::create_dir_all(root.join(".macc/catalog/nested")).unwrap();
        fs::write(root.join(".macc/macc.yaml"), "tools:\n  enabled: []\n").unwrap();
        fs::write(
            root.join(".macc/catalog/nested/item"),
            vec![b'x'; 512 * 1024],
        )
        .unwrap();
        let save = f
            .command(&["save", "snapshot", "--only", "config,catalogs", "--offline"])
            .output()
            .unwrap();
        assert!(
            save.status.success(),
            "{}",
            String::from_utf8_lossy(&save.stderr)
        );
        fs::write(
            root.join(".macc/macc.yaml"),
            "# local\ntools:\n  enabled: []\n",
        )
        .unwrap();
        f
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_macc"));
        cmd.current_dir(self.project.path())
            .env("HOME", self.home.path())
            .env("USERPROFILE", self.home.path())
            .args(args);
        cmd
    }
    fn restore(&self) -> Command {
        self.command(&["restore", "snapshot", "--yes", "--offline"])
    }
}

fn assert_failure(out: std::process::Output, path: &Path) -> String {
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(!out.status.success(), "{stderr}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("Restore completed successfully"));
    assert!(
        stderr.contains("catalogs") && stderr.contains(path.to_str().unwrap()),
        "{stderr}"
    );
    stderr
}

#[test]
fn conflicting_catalog_parent_is_rejected_before_config_restore() {
    let f = Fixture::new();
    let root = f.project.path();
    let parent = root.join(".macc/catalog/nested");
    fs::remove_dir_all(&parent).unwrap();
    fs::write(&parent, "occupied").unwrap();
    let message = assert_failure(f.restore().output().unwrap(), &parent);
    assert!(message.contains("expected directory"), "{message}");
    assert_eq!(
        fs::read_to_string(root.join(".macc/macc.yaml")).unwrap(),
        "# local\ntools:\n  enabled: []\n"
    );
    assert_eq!(fs::read_to_string(parent).unwrap(), "occupied");
}

#[cfg(unix)]
#[test]
fn catalog_write_failure_reports_completed_config_and_can_be_retried() {
    use std::os::unix::process::CommandExt;
    let f = Fixture::new();
    let root = f.project.path();
    let item = root.join(".macc/catalog/nested/item");
    fs::write(&item, "old catalog").unwrap();
    let mut cmd = f.restore();
    // Only this child has a file-size limit. The small config fits; the catalog does not.
    unsafe {
        cmd.pre_exec(|| {
            let cap = libc::rlimit {
                rlim_cur: 128 * 1024,
                rlim_max: 128 * 1024,
            };
            if libc::signal(libc::SIGXFSZ, libc::SIG_IGN) == libc::SIG_ERR
                || libc::setrlimit(libc::RLIMIT_FSIZE, &cap) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let message = assert_failure(cmd.output().unwrap(), &item);
    assert!(
        message.contains("Completed sections: configuration")
            && message.contains("Retry point: catalogs"),
        "{message}"
    );
    assert!(
        message.contains(&std::io::Error::from_raw_os_error(libc::EFBIG).to_string()),
        "{message}"
    );
    assert_eq!(
        fs::read_to_string(root.join(".macc/macc.yaml")).unwrap(),
        "tools:\n  enabled: []\n"
    );
    assert_eq!(fs::read_to_string(&item).unwrap(), "old catalog");
    let retry = f.restore().output().unwrap();
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    assert_eq!(fs::read(item).unwrap(), vec![b'x'; 512 * 1024]);
}

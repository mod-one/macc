//! Bounded command execution shared by isolated provider adapters.
use super::*;
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub(super) fn executable(name: &str) -> Result<std::path::PathBuf> {
    let p = Path::new(name);
    let candidates = if p.is_absolute() {
        vec![p.to_path_buf()]
    } else if p.components().count() == 1 {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .filter(|p| p.is_absolute())
            .map(|p| p.join(name))
            .collect()
    } else {
        return Err(error(
            "Provider executable must be absolute or a PATH basename",
        ));
    };
    for p in candidates {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if !p
                .metadata()
                .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
            {
                continue;
            }
        }
        if p.is_file() {
            return p.canonicalize().map_err(|e| error(e.to_string()));
        }
    }
    Err(error(format!("Provider executable missing: {name}")))
}
pub(super) fn run(
    exe: &Path,
    args: &[String],
    cwd: &Path,
    seconds: u64,
    memory_mb: u64,
    max_output: u64,
) -> Result<String> {
    std::fs::create_dir_all(cwd).map_err(|e| error(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(cwd, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| error(e.to_string()))?;
    }
    for dir in ["home", "cache", "tmp"] {
        std::fs::create_dir_all(cwd.join(dir)).map_err(|e| error(e.to_string()))?;
    }
    let log = cwd.join(format!("command-{}.log", uuid::Uuid::new_v4()));
    store::write(cwd, &log, b"")?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&log)
        .map_err(|e| error(e.to_string()))?;
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", cwd.join("home"))
        .env("XDG_CACHE_HOME", cwd.join("cache"))
        .env("TMPDIR", cwd.join("tmp"))
        .env("UV_CACHE_DIR", cwd.join("cache"))
        .env("UV_PYTHON_DOWNLOADS", "never")
        .env("UV_NO_PROGRESS", "1")
        .env("UV_NO_CONFIG", "1")
        .env("UV_KEYRING_PROVIDER", "disabled")
        .env("UV_CONCURRENT_DOWNLOADS", "1")
        .env("UV_CONCURRENT_INSTALLS", "1")
        .env("UV_CONCURRENT_BUILDS", "1")
        .env("RAYON_NUM_THREADS", "1")
        .env("TOKIO_WORKER_THREADS", "1")
        .env("MALLOC_ARENA_MAX", "2")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("OPENBLAS_NUM_THREADS", "1")
        .env("OMP_NUM_THREADS", "1")
        .env("GRAPHIFY_MAX_WORKERS", "1")
        .env("PYTHONHASHSEED", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            file.try_clone().map_err(|e| error(e.to_string()))?,
        ))
        .stderr(Stdio::from(file));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        unsafe {
            cmd.pre_exec(move || {
                #[cfg(target_os = "linux")]
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let memory = libc::rlimit {
                    rlim_cur: memory_mb * 1024 * 1024,
                    rlim_max: memory_mb * 1024 * 1024,
                };
                let cpu = libc::rlimit {
                    rlim_cur: seconds + 2,
                    rlim_max: seconds + 2,
                };
                if libc::setrlimit(libc::RLIMIT_AS, &memory) != 0
                    || libc::setrlimit(libc::RLIMIT_CPU, &cpu) != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = ChildGuard(cmd.spawn().map_err(|e| error(e.to_string()))?);
    let started = Instant::now();
    let status = loop {
        if let Some(s) = child.0.try_wait().map_err(|e| error(e.to_string()))? {
            break Some(s);
        }
        if started.elapsed() > Duration::from_secs(seconds)
            || std::fs::metadata(&log).map_or(true, |m| m.len() > max_output)
        {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.0.id() as i32), libc::SIGKILL);
    }
    if status.is_none() {
        let _ = child.0.kill();
        let _ = child.0.wait();
        return Err(error("Provider command exceeded time/output limit"));
    }
    let mut raw = Vec::new();
    std::fs::File::open(&log)
        .map_err(|e| error(e.to_string()))?
        .take(max_output + 1)
        .read_to_end(&mut raw)
        .map_err(|e| error(e.to_string()))?;
    if raw.len() as u64 > max_output {
        return Err(error("Provider command output exceeded limit"));
    }
    let text = String::from_utf8(raw).map_err(|e| error(e.to_string()))?;
    paths::safe_text(&text)?;
    if !status.unwrap().success() {
        return Err(error(format!(
            "Provider command failed: {}",
            text.chars().take(500).collect::<String>()
        )));
    }
    Ok(text)
}

struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn commands_are_bounded_and_credentials_are_not_inherited() {
        let t = tempfile::tempdir().unwrap();
        let output = run(
            Path::new("/bin/sh"),
            &[
                "-c".into(),
                "printf '%s|%s' \"$OPENAI_API_KEY\" \"$HOME\"".into(),
            ],
            t.path(),
            2,
            512,
            10000,
        )
        .unwrap();
        assert!(output.starts_with('|'));
        assert!(output.contains(&t.path().display().to_string()));
        assert!(run(
            Path::new("/bin/sh"),
            &["-c".into(), "sleep 20 & wait".into()],
            t.path(),
            1,
            512,
            10000
        )
        .unwrap_err()
        .to_string()
        .contains("limit"));
        assert!(run(
            Path::new("/bin/sh"),
            &["-c".into(), "yes bounded".into()],
            t.path(),
            2,
            512,
            1000
        )
        .is_err());
    }
}

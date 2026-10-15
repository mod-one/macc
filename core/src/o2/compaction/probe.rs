use super::*;
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub(super) fn executable(command: &str) -> Result<std::path::PathBuf> {
    let path = Path::new(command);
    let candidates = if path.is_absolute() {
        vec![path.to_path_buf()]
    } else if path.components().count() == 1 {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .filter(|p| p.is_absolute())
            .map(|p| p.join(command))
            .collect()
    } else {
        return Err(error(
            "Probe executable must be absolute or resolved through absolute PATH entries",
        ));
    };
    for p in candidates {
        if p.is_file() {
            let p = p.canonicalize().map_err(|e| error(e.to_string()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if std::fs::metadata(&p)
                    .map_err(|e| error(e.to_string()))?
                    .permissions()
                    .mode()
                    & 0o111
                    == 0
                {
                    continue;
                }
            }
            return Ok(p);
        }
    }
    Err(error("Tool executable missing"))
}
pub(super) fn binary_hash(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).map_err(|e| error(e.to_string()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let size = file.read(&mut buffer).map_err(|e| error(e.to_string()))?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub(super) fn run(root: &Path, command: &Path, arg: &str) -> Result<String> {
    let path = root.join(format!(".macc/cache/probe-{}.txt", uuid::Uuid::new_v4()));
    store::write(root, &path, b"")?;
    let result = (|| {
        let output = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .map_err(|e| error(e.to_string()))?;
        let mut c = Command::new(command);
        c.arg(arg)
            .current_dir(root)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("NO_COLOR", "1")
            .stdout(Stdio::from(
                output.try_clone().map_err(|e| error(e.to_string()))?,
            ))
            .stderr(Stdio::from(output));
        if let Some(home) = std::env::var_os("HOME") {
            c.env("HOME", home);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            c.process_group(0);
        }
        let mut child = c.spawn().map_err(|e| error(e.to_string()))?;
        let deadline = Instant::now() + Duration::from_secs(3);
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| error(e.to_string()))? {
                break Some(status);
            }
            if Instant::now() >= deadline
                || std::fs::metadata(&path)
                    .map(|m| m.len() > 65536)
                    .unwrap_or(true)
            {
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        #[cfg(unix)]
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        if status.is_none() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error("Capability probe timed out or exceeded output limit"));
        }
        if !status.unwrap().success() {
            return Err(error("Capability probe exited unsuccessfully"));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .map_err(|e| error(e.to_string()))?
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|e| error(e.to_string()))?;
        if bytes.len() > 65536 {
            return Err(error("Capability output too large"));
        }
        let text = String::from_utf8(bytes).map_err(|e| error(e.to_string()))?;
        super::super::paths::safe_text(&text)?;
        Ok(text)
    })();
    let _ = std::fs::remove_file(&path);
    result
}

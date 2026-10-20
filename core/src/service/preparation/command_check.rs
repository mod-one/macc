//! Bounded subprocess checks with explicit failure kinds and retained evidence.
use super::*;
use crate::tool::ToolInstallCommand;
use std::path::PathBuf;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FailureKind {
    Timeout,
    Launch,
    Exit,
    Wait,
    OutputLimit,
    InvalidResponse,
    Authentication,
    Network,
}
#[derive(Debug)]
pub(super) struct Failure {
    pub kind: FailureKind,
    pub path: PathBuf,
    pub reason: String,
}
impl Failure {
    pub fn new(kind: FailureKind, path: &Path, reason: String) -> Self {
        Self {
            kind,
            path: path.into(),
            reason,
        }
    }
    pub fn classify(mut self, spec: &crate::tool::preparation::PreparationSpec) -> Self {
        // Transport/process failures must never trigger account setup.
        if self.kind != FailureKind::Exit {
            return self;
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        if let Ok(file) = std::fs::File::open(&self.path) {
            let _ = file.take(65536).read_to_end(&mut bytes);
        }
        let raw = String::from_utf8_lossy(&bytes);
        let output = raw.to_ascii_lowercase();
        let authentication_json = serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .is_some_and(|value| {
                spec.authentication_json
                    .iter()
                    .any(|(p, v)| value.pointer(p) == Some(v))
            });
        let matches = |patterns: &[String]| {
            patterns
                .iter()
                .any(|p| !p.is_empty() && output.contains(&p.to_ascii_lowercase()))
        };
        if matches(&spec.network_errors) {
            self.kind = FailureKind::Network;
        } else if matches(&spec.authentication_errors) || authentication_json {
            self.kind = FailureKind::Authentication;
        }
        self
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Tool readiness {:?}: {}; evidence: {}",
            self.kind,
            self.reason,
            self.path.display()
        )?;
        use std::io::{Read, Seek, SeekFrom};
        if self
            .path
            .extension()
            .is_none_or(|extension| extension != "log")
        {
            return Ok(());
        }
        if let Ok(mut file) = std::fs::File::open(&self.path) {
            if let Ok(meta) = file.metadata() {
                let _ = file.seek(SeekFrom::Start(meta.len().saturating_sub(2048)));
                let mut raw = Vec::new();
                let _ = file.take(2048).read_to_end(&mut raw);
                let text = crate::save::redact_secrets_in_text(&String::from_utf8_lossy(&raw));
                let text: String = text
                    .chars()
                    .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
                    .collect();
                if !text.trim().is_empty() {
                    write!(f, "\nDiagnostic: {}", text.trim())?;
                }
            }
        }
        Ok(())
    }
}
impl From<Failure> for MaccError {
    fn from(f: Failure) -> Self {
        Self::Validation(f.to_string())
    }
}
impl From<MaccError> for Failure {
    fn from(e: MaccError) -> Self {
        Self::new(
            FailureKind::Launch,
            Path::new(".macc/log/apply"),
            e.to_string(),
        )
    }
}
pub(super) fn run(
    paths: &ProjectPaths,
    command: &ToolInstallCommand,
    name: &str,
    timeout: u64,
) -> std::result::Result<std::path::PathBuf, Failure> {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    if name
        .chars()
        .any(|c| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
    {
        return Err(Failure::new(
            FailureKind::Launch,
            &paths.macc_dir.join("log/apply"),
            "Invalid Apply operation name".into(),
        ));
    }
    let directory = super::progress::directory(paths);
    std::fs::create_dir_all(&directory).map_err(|e| MaccError::Validation(e.to_string()))?;
    let path = directory.join(format!("{}-{name}.log", uuid::Uuid::new_v4()));
    crate::atomic_write(
        paths,
        &paths.macc_dir.join("state/apply-current-log"),
        path.strip_prefix(paths.macc_dir.join("log/apply"))
            .unwrap()
            .to_string_lossy()
            .as_bytes(),
    )?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(&path)
        .map_err(|e| MaccError::Validation(e.to_string()))?;
    let mut process = Command::new(&command.command);
    process
        .args(&command.args)
        .current_dir(&paths.root)
        .stdin(Stdio::null())
        .stdout(
            file.try_clone()
                .map_err(|e| MaccError::Validation(e.to_string()))?,
        )
        .stderr(file);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }
    let mut child = process.spawn().map_err(|e| {
        Failure::new(
            FailureKind::Launch,
            &path,
            format!("{}: {e}", command.command),
        )
    })?;
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let result = loop {
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > 10 * 1024 * 1024) {
            break Err((FailureKind::OutputLimit, "output exceeded 10 MiB".into()));
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err((FailureKind::Exit, format!("{}: {status}", command.command)))
                }
            }
            Ok(None) => {}
            Err(e) => break Err((FailureKind::Wait, e.to_string())),
        }
        if Instant::now() >= deadline {
            break Err((
                FailureKind::Timeout,
                format!("{} exceeded {timeout} seconds", command.command),
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.wait();
    result
        .map(|_| path.clone())
        .map_err(|(kind, reason)| Failure::new(kind, &path, reason))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn command(script: &str) -> ToolInstallCommand {
        ToolInstallCommand {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
        }
    }
    fn spec() -> crate::tool::preparation::PreparationSpec {
        serde_json::from_value(serde_json::json!({"verify":{"command":"/bin/sh","args":[]},"authentication_errors":["login required"],"network_errors":["connection refused"]})).unwrap()
    }
    #[test]
    fn launch_exit_and_timeout_keep_distinct_causes_and_evidence() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        let failure = run(
            &paths,
            &ToolInstallCommand {
                command: tmp.path().join("missing").display().to_string(),
                args: vec![],
            },
            "launch",
            1,
        )
        .unwrap_err();
        assert_eq!(failure.kind, FailureKind::Launch);
        assert!(failure.path.is_file());
        let failure = run(
            &paths,
            &command("echo 'connection refused' >&2; exit 17"),
            "exit",
            1,
        )
        .unwrap_err()
        .classify(&spec());
        assert_eq!(failure.kind, FailureKind::Network);
        assert!(failure.reason.contains("17"));
        assert!(failure.to_string().contains("evidence:"));
        assert!(failure
            .to_string()
            .contains("Diagnostic: connection refused"));
        let failure = run(&paths, &command("sleep 2"), "timeout", 0)
            .unwrap_err()
            .classify(&spec());
        assert_eq!(failure.kind, FailureKind::Timeout);
    }
    #[test]
    fn only_explicit_authentication_evidence_selects_authentication() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("check.log");
        std::fs::write(&log, "login required").unwrap();
        assert_eq!(
            Failure::new(FailureKind::Exit, &log, "exit 1".into())
                .classify(&spec())
                .kind,
            FailureKind::Authentication
        );
        assert_eq!(
            Failure::new(FailureKind::Timeout, &log, "timeout".into())
                .classify(&spec())
                .kind,
            FailureKind::Timeout
        );
        std::fs::write(&log, "unexpected failure").unwrap();
        assert_eq!(
            Failure::new(FailureKind::Exit, &log, "exit 1".into())
                .classify(&spec())
                .kind,
            FailureKind::Exit
        );
    }
}

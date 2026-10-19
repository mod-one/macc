use super::*;
use crate::doctor::{check_tool, SystemRunner, ToolStatus};
use crate::tool::{DoctorCheckKind, ToolInstallCommand, ToolSpec, ToolSpecLoader};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    pub spec: ToolSpec,
    pub install_needed: bool,
}
fn available(spec: &ToolSpec) -> bool {
    spec.performer.as_ref().is_some_and(|p| {
        matches!(
            check_tool(&SystemRunner, &DoctorCheckKind::Which, &p.command),
            ToolStatus::Installed
        )
    })
}
pub(super) fn requirements(
    paths: &ProjectPaths,
    config: &crate::config::CanonicalConfig,
) -> Result<Vec<Requirement>> {
    let (specs, diagnostics) =
        ToolSpecLoader::new(ToolSpecLoader::default_search_paths(&paths.root))
            .load_all_with_embedded();
    let mut selected = config.tools.enabled.clone();
    if let Some(tool) = config
        .automation
        .supervisor
        .as_ref()
        .and_then(|s| s.tool.as_ref())
    {
        selected.push(tool.clone());
    }
    if let Some(tool) = config
        .automation
        .coordinator
        .as_ref()
        .and_then(|s| s.coordinator_tool.as_ref())
    {
        selected.push(tool.clone());
    }
    if diagnostics
        .iter()
        .any(|d| d.path.starts_with(&paths.macc_dir))
    {
        return Err(MaccError::Validation(
            "Resolve invalid project ToolSpec before Apply".into(),
        ));
    }
    Ok(specs
        .into_iter()
        .filter(|s| selected.contains(&s.id))
        .map(|spec| {
            let install_needed = !available(&spec);
            Requirement {
                spec,
                install_needed,
            }
        })
        .collect())
}
pub(super) fn prepare(
    paths: &ProjectPaths,
    requirement: &Requirement,
    allow_setup: bool,
) -> Result<()> {
    let spec = &requirement.spec;
    if !available(spec) {
        if !requirement.install_needed {
            return Err(MaccError::Validation(format!(
                "{} became unavailable after review; refresh Apply",
                spec.id
            )));
        }
        let install = spec
            .install
            .as_ref()
            .filter(|i| !i.commands.is_empty())
            .ok_or_else(|| {
                MaccError::Validation(format!(
                    "{} is missing and its adapter has no automatic installer",
                    spec.id
                ))
            })?;
        let config = crate::load_canonical_config(&paths.config_path)?;
        if config.settings.offline {
            return Err(MaccError::Validation(format!(
                "{} installation needs network access; change Offline in Apply configuration",
                spec.id
            )));
        }
        for (i, command) in install.commands.iter().enumerate() {
            run(paths, command, &format!("{}-install-{i}", spec.id), 600)?;
        }
    }
    if !available(spec) {
        return Err(MaccError::Validation(format!(
            "{} is still unavailable after setup",
            spec.id
        )));
    }
    if let Some(version) = &spec.version_check {
        run(paths, &version.current, &format!("{}-verify", spec.id), 15)?;
    }
    if spec.performer.is_none() {
        return Err(MaccError::Validation(format!(
            "{} has no performer adapter",
            spec.id
        )));
    }
    if let Some(preparation) = spec.performer.as_ref().and_then(|p| p.preparation.as_ref()) {
        if verify(paths, &spec.id, preparation).is_err() {
            if !allow_setup {
                return Err(MaccError::Validation(format!(
                    "{} account/runtime is not ready; complete Apply account setup",
                    spec.id
                )));
            }
            let setup = preparation.setup.as_ref().ok_or_else(|| {
                MaccError::Validation(format!(
                    "{} requires account setup; its adapter has no automated setup flow",
                    spec.id
                ))
            })?;
            if crate::load_canonical_config(&paths.config_path)?
                .settings
                .offline
            {
                return Err(MaccError::Validation(
                    "Account setup requires network access in Apply".into(),
                ));
            }
            run(paths, setup, &format!("{}-account", spec.id), 600)?;
            verify(paths, &spec.id, preparation)?;
        }
    }
    Ok(())
}
fn verify(
    paths: &ProjectPaths,
    id: &str,
    spec: &crate::tool::preparation::PreparationSpec,
) -> Result<()> {
    let path = run(paths, &spec.verify, &format!("{id}-account-check"), 15)?;
    if !spec.expected_json.is_empty() {
        let raw = std::fs::read(&path).map_err(|e| MaccError::Validation(e.to_string()))?;
        if raw.len() > 65536 {
            return Err(MaccError::Validation("Readiness JSON exceeds bound".into()));
        }
        let start = raw.iter().position(|c| *c == b'{').unwrap_or(0);
        let value = serde_json::Deserializer::from_slice(&raw[start..])
            .into_iter::<serde_json::Value>()
            .next()
            .ok_or_else(|| MaccError::Validation("Readiness JSON missing".into()))?
            .map_err(|e| MaccError::Validation(e.to_string()))?;
        for (pointer, expected) in &spec.expected_json {
            if value.pointer(pointer) != Some(expected) {
                return Err(MaccError::Validation(format!(
                    "{id} readiness predicate {pointer} is not satisfied"
                )));
            }
        }
    }
    Ok(())
}
fn run(
    paths: &ProjectPaths,
    command: &ToolInstallCommand,
    name: &str,
    timeout: u64,
) -> Result<std::path::PathBuf> {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    if name
        .chars()
        .any(|c| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
    {
        return Err(MaccError::Validation("Invalid Apply operation name".into()));
    }
    let directory = paths.macc_dir.join("log/apply");
    std::fs::create_dir_all(&directory).map_err(|e| MaccError::Validation(e.to_string()))?;
    let path = directory.join(format!("{}-{name}.log", uuid::Uuid::new_v4()));
    crate::atomic_write(
        paths,
        &paths.macc_dir.join("state/apply-current-log"),
        path.file_name().unwrap().to_string_lossy().as_bytes(),
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
    let mut child = process
        .spawn()
        .map_err(|e| MaccError::Validation(format!("{}: {e}", command.command)))?;
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err(format!("exit {status}"))
                }
            }
            Ok(None) => {}
            Err(e) => break Err(e.to_string()),
        }
        if Instant::now() >= deadline
            || std::fs::metadata(&path).is_ok_and(|m| m.len() > 10 * 1024 * 1024)
        {
            break Err("timeout/output limit exceeded".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.wait();
    result.map(|_|path.clone()).map_err(|reason|MaccError::Validation(format!("Apply preparation {} failed ({reason}); evidence: {}. Resolve the requested setup in Apply and retry.",command.command,path.display())))
}

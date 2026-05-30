use macc_core::config::CanonicalConfig;
use macc_core::{load_canonical_config, ProjectPaths, Result};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub mod apply;
pub mod backups;
pub mod catalog;
pub mod catalog_support;
pub mod clear;
pub mod config;
pub mod context;
pub mod coordinator;
pub mod doctor;
pub mod init;
pub mod install;
pub mod lifecycle_support;
pub mod logs;
pub mod migrate;
pub mod plan;
pub mod process;
pub mod quickstart;
pub mod restore;
pub mod supervisor;
pub mod tool;
pub mod web;
pub mod worktree;
pub mod start;
pub mod trust;
pub mod lock;
pub mod failure;
pub mod settings;
pub mod save;

pub trait Command {
    fn run(&self) -> Result<()>;
}

/// L6-OWN-008: CLI-side mutation gate. If the project-wide control lease has an
/// owner that is not the local CLI identity, the action is rejected.
/// Read-only / one-shot CLI commands (plan, status, etc.) skip this.
///
/// Control-plane internal subprocesses (the coordinator spawning `macc worktree
/// apply`, the supervisor spawning the coordinator, etc.) set
/// `MACC_INTERNAL_INVOCATION=1` to bypass the gate. The parent process has
/// already been authorized by the owner; its children act on the same
/// authority.
pub fn gate_cli_mutation(project_root: &std::path::Path) -> Result<()> {
    use macc_core::process_ownership::{ProcessHandle, ProcessKind};
    use macc_core::service::process_ownership_gate::{gate_owner_action, ClientContext};

    if std::env::var("MACC_INTERNAL_INVOCATION")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        return Ok(());
    }

    let client_id =
        std::env::var("MACC_CLIENT_ID").unwrap_or_else(|_| format!("cli-{}", std::process::id()));
    gate_owner_action(
        &ClientContext {
            client_id,
            project_root: project_root.to_path_buf(),
        },
        &ProcessHandle {
            kind: ProcessKind::Project,
            project_root: project_root.to_path_buf(),
            pid: None,
        },
    )
}

#[derive(Clone)]
pub struct AppContext {
    pub cwd: PathBuf,
    pub engine: crate::services::engine_provider::SharedEngine,
    pub overrides: macc_core::resolve::CliOverrides,
    cache: Arc<AppContextCache>,
}

#[derive(Default)]
struct AppContextCache {
    project_paths: Mutex<Option<ProjectPaths>>,
    canonical: Mutex<Option<CanonicalConfig>>,
}

impl AppContext {
    pub fn new(
        cwd: PathBuf,
        engine: crate::services::engine_provider::SharedEngine,
        overrides: macc_core::resolve::CliOverrides,
    ) -> Self {
        Self {
            cwd,
            engine,
            overrides,
            cache: Arc::new(AppContextCache::default()),
        }
    }

    pub fn project_paths(&self) -> Result<ProjectPaths> {
        if let Some(cached) = self
            .cache
            .project_paths
            .lock()
            .map_err(|_| macc_core::MaccError::Validation("project cache lock poisoned".into()))?
            .clone()
        {
            return Ok(cached);
        }

        let paths = macc_core::find_project_root(&self.cwd)?;
        let mut guard =
            self.cache.project_paths.lock().map_err(|_| {
                macc_core::MaccError::Validation("project cache lock poisoned".into())
            })?;
        *guard = Some(paths.clone());
        Ok(paths)
    }

    pub fn ensure_initialized_paths(&self) -> Result<ProjectPaths> {
        let paths = self.engine.project_ensure_initialized_paths(&self.cwd)?;
        let mut guard =
            self.cache.project_paths.lock().map_err(|_| {
                macc_core::MaccError::Validation("project cache lock poisoned".into())
            })?;
        *guard = Some(paths.clone());
        Ok(paths)
    }

    pub fn canonical_config(&self) -> Result<CanonicalConfig> {
        if let Some(cached) = self
            .cache
            .canonical
            .lock()
            .map_err(|_| macc_core::MaccError::Validation("canonical cache lock poisoned".into()))?
            .clone()
        {
            return Ok(cached);
        }

        let paths = self.project_paths()?;
        let canonical = load_canonical_config(&paths.config_path)?;
        let mut guard = self.cache.canonical.lock().map_err(|_| {
            macc_core::MaccError::Validation("canonical cache lock poisoned".into())
        })?;
        *guard = Some(canonical.clone());
        Ok(canonical)
    }
}

pub fn create_save_bundle_interactive(
    paths: &macc_core::ProjectPaths,
    name: &str,
    mut opts: macc_core::save::SaveOptions,
) -> Result<macc_core::save::SaveBundleManifest> {
    loop {
        match macc_core::save::create_save_bundle(paths, name, &opts) {
            Err(macc_core::MaccError::Validation(ref msg)) if msg.contains("MACC-SAVE-1003") => {
                let term_interactive = !cfg!(test) || std::env::var("MACC_FORCE_INTERACTIVE").is_ok();
                if !term_interactive {
                    return Err(macc_core::MaccError::Validation(msg.clone()));
                }

                println!("Potential secrets were detected in files selected for saving.\n");
                println!("Choose:");
                println!("  [R] Save with redaction if possible");
                println!("  [E] Exclude affected files");
                println!("  [A] Abort");

                use std::io::{self, Write};
                print!("> ");
                io::stdout().flush().ok();
                let mut input = String::new();
                io::stdin().read_line(&mut input).ok();
                let choice = input.trim().to_lowercase();

                if choice == "r" || choice == "redact" {
                    opts.handle_secrets = Some(macc_core::save::SecretHandlingChoice::Redact);
                } else if choice == "e" || choice == "exclude" {
                    opts.handle_secrets = Some(macc_core::save::SecretHandlingChoice::Exclude);
                } else {
                    println!("Save aborted.");
                    return Err(macc_core::MaccError::Validation(msg.clone()));
                }
            }
            res => return res,
        }
    }
}

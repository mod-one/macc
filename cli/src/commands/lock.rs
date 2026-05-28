use crate::commands::{AppContext, Command};
use macc_core::Result;
use macc_core::ops_motif::{generate_lock_manifest, verify_lock_manifest, LockManifest};
use std::fs;

pub struct LockCommand {
    app: AppContext,
    subcommand: LockCommands,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum LockCommands {
    /// Resolves and writes/updates the lock file macc.lock.yaml
    Generate,
    /// Verifies current environment against the lock file
    Check,
    /// Shows drift between current environment and lock
    Diff,
    /// Human-readable explanation of what is pinned and why
    Explain,
}

impl LockCommand {
    pub fn new(app: AppContext, subcommand: LockCommands) -> Self {
        Self { app, subcommand }
    }
}

impl Command for LockCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;
        let lock_path = paths.macc_dir.join("macc.lock.yaml");

        match self.subcommand {
            LockCommands::Generate => {
                println!("Generating lock manifest...");
                let config = self.app.canonical_config()?;
                let lock = generate_lock_manifest(&paths, &config)?;
                let serialized = serde_yaml::to_string(&lock)
                    .map_err(|e| macc_core::MaccError::Validation(format!("serialize lock manifest: {}", e)))?;
                fs::write(&lock_path, serialized)
                    .map_err(|e| macc_core::MaccError::Io {
                        path: lock_path.to_string_lossy().into(),
                        action: "write macc.lock.yaml".into(),
                        source: e,
                    })?;
                println!("Locked environment saved to {}", lock_path.display());
            }
            LockCommands::Check => {
                if !lock_path.exists() {
                    return Err(macc_core::MaccError::Validation(format!(
                        "Lock file not found. Run 'macc lock generate' first."
                    )));
                }
                println!("Checking environment lock integrity...");
                let lock_str = fs::read_to_string(&lock_path)
                    .map_err(|e| macc_core::MaccError::Io {
                        path: lock_path.to_string_lossy().into(),
                        action: "read macc.lock.yaml".into(),
                        source: e,
                    })?;
                let lock: LockManifest = serde_yaml::from_str(&lock_str)
                    .map_err(|e| macc_core::MaccError::Config {
                        path: lock_path.to_string_lossy().into(),
                        source: e,
                    })?;
                let report = verify_lock_manifest(&paths, &lock)?;
                if report.matches {
                    println!("Lock verification: SUCCESS. Environment matches lock perfectly.");
                } else {
                    println!("Lock verification: DRIFT DETECTED.");
                    for d in report.drift {
                        println!("  - {}", d);
                    }
                    return Err(macc_core::MaccError::Validation("Lock file check failed due to drift.".to_string()));
                }
            }
            LockCommands::Diff => {
                if !lock_path.exists() {
                    println!("Lock file does not exist yet. Current environment represents 100% drift.");
                    return Ok(());
                }
                let lock_str = fs::read_to_string(&lock_path).unwrap_or_default();
                let lock: LockManifest = serde_yaml::from_str(&lock_str).unwrap();
                let report = verify_lock_manifest(&paths, &lock)?;
                if report.matches {
                    println!("No difference between lock and current environment.");
                } else {
                    println!("Current drift from lock:");
                    for d in report.drift {
                        println!("  - {}", d);
                    }
                }
            }
            LockCommands::Explain => {
                println!("MACC lock manifest pins version and checksum parameters:");
                println!("- lock_version: format version");
                println!("- config_sha256: lock configuration to prevent configuration changes");
                println!("- tools: list of resolved AI coding tools and their generated file hashes");
                println!("- catalogs: source repository commits to guarantee catalog integrity");
            }
        }

        Ok(())
    }
}

use crate::commands::AppContext;
use crate::commands::Command;
use macc_core::Result;

pub struct ApplyCommand {
    app: AppContext,
    tools: Option<String>,
    dry_run: bool,
    allow_user_scope: bool,
    json: bool,
    explain: bool,
    locked: bool,
}

impl ApplyCommand {
    pub fn new(
        app: AppContext,
        tools: Option<String>,
        dry_run: bool,
        allow_user_scope: bool,
        json: bool,
        explain: bool,
        locked: bool,
    ) -> Self {
        Self {
            app,
            tools,
            dry_run,
            allow_user_scope,
            json,
            explain,
            locked,
        }
    }
}

fn validate_locked(paths: &macc_core::ProjectPaths) -> Result<()> {
    let lock_path = paths.macc_dir.join("macc.lock.yaml");
    if !lock_path.exists() {
        return Err(macc_core::MaccError::Validation(
            "Lock file not found. Cannot proceed under --locked.".to_string(),
        ));
    }
    let lock_str = std::fs::read_to_string(&lock_path)
        .map_err(|e| macc_core::MaccError::Io {
            path: lock_path.to_string_lossy().into(),
            action: "read macc.lock.yaml".into(),
            source: e,
        })?;
    let lock: macc_core::ops_motif::LockManifest = serde_yaml::from_str(&lock_str)
        .map_err(|e| macc_core::MaccError::Config {
            path: lock_path.to_string_lossy().into(),
            source: e,
        })?;
    let report = macc_core::ops_motif::verify_lock_manifest(paths, &lock)?;
    if !report.matches {
        eprintln!("Lock verification: DRIFT DETECTED.");
        for d in report.drift {
            eprintln!("  - {}", d);
        }
        return Err(macc_core::MaccError::Validation(
            "Lock file check failed due to drift under --locked constraint.".to_string(),
        ));
    }
    Ok(())
}

impl Command for ApplyCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;
        if self.locked {
            validate_locked(&paths)?;
        }

        // L6-OWN-008: gate `apply` against the project-wide control lease.
        // Dry-runs are read-only; skip the gate for those.
        if !self.dry_run {
            crate::commands::gate_cli_mutation(&paths.root)?;
        }
        crate::commands::lifecycle_support::apply(
            &self.app,
            self.tools.as_deref(),
            self.dry_run,
            self.allow_user_scope,
            self.json,
            self.explain,
        )
    }
}

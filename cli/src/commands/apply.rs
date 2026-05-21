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
}

impl ApplyCommand {
    pub fn new(
        app: AppContext,
        tools: Option<String>,
        dry_run: bool,
        allow_user_scope: bool,
        json: bool,
        explain: bool,
    ) -> Self {
        Self {
            app,
            tools,
            dry_run,
            allow_user_scope,
            json,
            explain,
        }
    }
}

impl Command for ApplyCommand {
    fn run(&self) -> Result<()> {
        // L6-OWN-008: gate `apply` against the project-wide control lease.
        // Dry-runs are read-only; skip the gate for those.
        if !self.dry_run {
            if let Ok(paths) = self.app.project_paths() {
                crate::commands::gate_cli_mutation(&paths.root)?;
            }
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

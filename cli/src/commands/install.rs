use crate::commands::catalog_support::{CliCatalogInstallBackend, CliCatalogUi};
use crate::commands::AppContext;
use crate::commands::Command;
use crate::InstallCommands;
use macc_core::Result;

pub struct InstallCommand<'a> {
    app: AppContext,
    command: &'a InstallCommands,
}

impl<'a> InstallCommand<'a> {
    pub fn new(app: AppContext, command: &'a InstallCommands) -> Self {
        Self { app, command }
    }
}

impl<'a> Command for InstallCommand<'a> {
    fn run(&self) -> Result<()> {
        if let InstallCommands::Skill { tool, id } = self.command {
            return super::skills_install::install(
                &self.app,
                super::skills_install::Request {
                    id: id.clone(),
                    tool: tool.clone(),
                    reference: None,
                    source: None,
                },
                false,
            );
        }
        let paths = self.app.project_paths()?;
        let dummy_plan = macc_core::plan::ActionPlan::new();
        macc_core::ops_motif::print_trust_review_card(&paths, &dummy_plan, false);
        if !crate::confirm_yes_no("Proceed with catalog installation [y/N]? ")? {
            println!("Installation cancelled.");
            return Ok(());
        }
        let backend = CliCatalogInstallBackend { app: &self.app };
        match self.command {
            InstallCommands::Skill { .. } => unreachable!("handled by shared skill lifecycle"),
            InstallCommands::Mcp { id } => {
                self.app
                    .engine
                    .catalog_install_mcp(&paths, id, &backend, &CliCatalogUi)
            }
        }
    }
}

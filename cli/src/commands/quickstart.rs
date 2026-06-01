use crate::commands::AppContext;
use crate::commands::Command;
use macc_core::Result;

pub struct QuickstartCommand {
    app: AppContext,
    yes: bool,
    apply: bool,
    no_tui: bool,
    tool: Option<String>,
    starter_task: bool,
    start_coordinator: bool,
    check_only: bool,
    json: bool,
}

impl QuickstartCommand {
    pub fn new(
        app: AppContext,
        yes: bool,
        apply: bool,
        no_tui: bool,
        tool: Option<String>,
        starter_task: bool,
        start_coordinator: bool,
        check_only: bool,
        json: bool,
    ) -> Self {
        Self {
            app,
            yes,
            apply,
            no_tui,
            tool,
            starter_task,
            start_coordinator,
            check_only,
            json,
        }
    }
}

impl Command for QuickstartCommand {
    fn run(&self) -> Result<()> {
        crate::commands::lifecycle_support::quickstart_extended(
            &self.app,
            self.yes,
            self.apply,
            self.no_tui,
            self.tool.as_deref(),
            self.starter_task,
            self.start_coordinator,
            self.check_only,
            self.json,
        )
    }
}

use crate::commands::AppContext;
use crate::commands::Command;
use macc_core::Result;

pub struct WebCommand {
    _app: AppContext,
}

impl WebCommand {
    pub fn new(app: AppContext) -> Self {
        Self { _app: app }
    }
}

impl Command for WebCommand {
    fn run(&self) -> Result<()> {
        println!("Web server starting...");
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Tools,
    Automation,
    CoordinatorLive,
    Mcp,
    Logs,
    Skills,
    Hooks,
    Agents,
    ToolSettings,
    Preview,
    Apply,
    Settings,
    Sessions,
    O2,
    About,
    Watch,
}

impl Screen {
    pub fn title(&self) -> &'static str {
        match self {
            Screen::Home => "Home",
            Screen::Tools => "Tools Configuration",
            Screen::Automation => "Automation / Coordinator",
            Screen::CoordinatorLive => "Coordinator Live",
            Screen::Mcp => "MCP Servers",
            Screen::Logs => "Logs",
            Screen::Skills => "Skills",
            Screen::Hooks => "Hooks",
            Screen::Agents => "Agents",
            Screen::ToolSettings => "Tool Settings",
            Screen::Preview => "Preview",
            Screen::Apply => "Apply Changes",
            Screen::Settings => "Global Settings",
            Screen::Sessions => "Sessions & Conversations",
            Screen::O2 => "O2 Memory & Context",
            Screen::About => "About",
            Screen::Watch => "Observer",
        }
    }

    pub fn help_keybindings(&self) -> Vec<(&'static str, &'static str)> {
        let mut commands = crate::command_catalog::screen_action_bindings(*self);
        commands.extend([
            ("F1 / ?", "Help"),
            ("F2 / : / Ctrl-P", "Commands"),
            ("Esc", "Back / cancel"),
        ]);
        commands
    }
}

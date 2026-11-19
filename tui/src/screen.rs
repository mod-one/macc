#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Inbox,
    Changes,
    Doctor,
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
    Sessions,
    O2,
    About,
    Watch,
}

impl Screen {
    pub fn title(&self) -> &'static str {
        match self {
            Screen::Inbox => "Inbox",
            Screen::Changes => "Changes",
            Screen::Doctor => "Doctor",
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
            Screen::Sessions => "Sessions & Conversations",
            Screen::O2 => "O2 Memory & Context",
            Screen::About => "About",
            Screen::Watch => "Observer",
        }
    }
}

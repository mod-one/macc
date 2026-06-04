#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Tools,
    Automation,
    CoordinatorLive,
    Mcp,
    Logs,
    Skills,
    Agents,
    ToolSettings,
    Preview,
    Apply,
    Settings,
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
            Screen::Agents => "Agents",
            Screen::ToolSettings => "Tool Settings",
            Screen::Preview => "Preview",
            Screen::Apply => "Apply Changes",
            Screen::Settings => "Global Settings",
            Screen::About => "About",
            Screen::Watch => "Observer",
        }
    }

    pub fn help_keybindings(&self) -> Vec<(&'static str, &'static str)> {
        let mut bindings = vec![
            ("?", "Toggle Help"),
            ("q/Esc", "Back / Quit"),
            ("h", "Go Home"),
            ("t", "Go to Tools"),
            ("o", "Go to Automation"),
            ("v", "Go to Coordinator Live"),
            ("m", "Go to MCP"),
            ("g", "Go to Logs"),
            ("e", "Go to Settings"),
            ("p", "Go to Preview"),
            ("x", "Go to Apply"),
            ("s", "Save Config"),
            ("/", "Search / Filter"),
            ("u / U", "Undo / Redo config"),
        ];

        let screen_bindings = match self {
            Screen::Home => vec![
                ("d", "Run doctor check"),
                ("a", "Open Apply screen"),
                ("r", "Start coordinator + go to Live"),
                ("v", "Go to Coordinator Live"),
            ],
            Screen::Tools => vec![
                ("↑↓", "Navigate Tools"),
                ("Space", "Toggle Tool"),
                ("Enter", "Configure Tool"),
                ("d", "Refresh Tool Checks"),
                ("f", "Generate Tool Context"),
            ],
            Screen::Automation => {
                vec![("↑↓", "Navigate Settings"), ("Space/Enter", "Edit / Cycle")]
            }
            Screen::CoordinatorLive => vec![
                ("r", "Run Full Cycle"),
                ("y", "Sync Registry"),
                ("c", "Reconcile"),
                ("u", "Resume Paused Run"),
                ("k", "Stop Coordinator"),
                ("l", "Refresh Live Status"),
                ("T", "Request Takeover"),
                ("a/r", "Accept / Reject Takeover"),
                ("r/Enter (on error)", "Retry failed phase + resume"),
                ("s (on error)", "Skip failed phase + resume"),
                ("o (on error)", "Open logs"),
                ("k/Esc (on error)", "Stop after pause"),
            ],
            Screen::ToolSettings => vec![
                ("↑↓", "Navigate Fields"),
                ("Space/Enter", "Cycle Value / Edit Text/Number/Array"),
            ],
            Screen::Skills => vec![
                ("↑↓", "Navigate Skills"),
                ("Space/Enter", "Toggle Skill"),
                ("a", "Select All"),
                ("n", "Select None"),
            ],
            Screen::Agents => vec![
                ("↑↓", "Navigate Agents"),
                ("Space/Enter", "Toggle Agent"),
                ("a", "Select All"),
                ("n", "Select None"),
            ],
            Screen::Mcp => vec![
                ("↑↓", "Navigate MCP Templates"),
                ("Space/Enter", "Toggle Template"),
                ("a", "Select All"),
                ("n", "Select None"),
            ],
            Screen::Logs => vec![
                ("↑↓", "Select Log File"),
                ("PgUp/PgDn", "Scroll Log Content"),
                ("r", "Refresh Log List"),
                ("/", "Filter logs"),
            ],
            Screen::Preview => vec![
                ("↑↓", "Navigate Operations"),
                ("PgUp/PgDn", "Scroll Diff"),
                ("r", "Refresh Plan"),
                ("x", "Go to Apply Screen"),
            ],
            Screen::Apply => vec![
                ("Enter", "Apply Changes"),
                ("Backspace", "Delete last char of 'YES'"),
                ("YES", "Type to consent to user-scope ops"),
            ],
            Screen::Settings => vec![("↑↓", "Navigate Settings"), ("Space/Enter", "Edit / Cycle")],
            Screen::Watch => vec![
                ("↑↓", "Scroll workers"),
                ("f", "Follow / unfollow log"),
                ("/", "Search"),
                ("e", "Errors only"),
                ("w", "Warnings only"),
                ("a", "All logs"),
                ("r", "Refresh"),
                ("l", "Reload snapshot"),
            ],
            _ => vec![],
        };

        bindings.extend(screen_bindings);
        bindings
    }
}

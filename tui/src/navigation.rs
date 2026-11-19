//! Mode navigation and routes also used by the command palette.
use crate::{screen::Screen, session_navigation, state::AppState};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Operate,
    Configure,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Destination {
    Overview,
    Live,
    Workers,
    Inbox,
    Sessions,
    Memory,
    Logs,
    Doctor,
    Changes,
    General,
    Coordinator,
    Tools,
    Skills,
    Agents,
    Mcp,
    Hooks,
    MemorySettings,
    SessionSettings,
    About,
}
use Destination::*;
pub(crate) const ALL: &[Destination] = &[
    Overview,
    Live,
    Workers,
    Inbox,
    Sessions,
    Memory,
    Logs,
    Doctor,
    Changes,
    General,
    Coordinator,
    Tools,
    Skills,
    Agents,
    Mcp,
    Hooks,
    MemorySettings,
    SessionSettings,
    About,
];
impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Operate => "Operate",
            Self::Configure => "Configure",
        }
    }
    pub fn other_label(self) -> &'static str {
        if self == Self::Operate {
            "Switch to Configure"
        } else {
            "Switch to Operate"
        }
    }
    pub fn routes(self) -> &'static [(char, Destination)] {
        match self {
            Self::Operate => &[
                ('H', Overview),
                ('C', Live),
                ('I', Inbox),
                ('S', Sessions),
                ('O', Memory),
                ('L', Logs),
                ('D', Doctor),
            ],
            Self::Configure => &[
                ('H', Changes),
                ('G', General),
                ('C', Coordinator),
                ('T', Tools),
                ('K', Skills),
                ('M', Mcp),
                ('P', Hooks),
                ('O', MemorySettings),
                ('S', SessionSettings),
            ],
        }
    }
}
impl Destination {
    pub fn label(self) -> &'static str {
        match self {
            Overview => "Overview",
            Live => "Coordinator live",
            Workers => "Workers",
            Inbox => "Inbox",
            Sessions => "Sessions",
            Memory => "O2 Memory",
            Logs => "Logs",
            Doctor => "Doctor",
            Changes => "Changes",
            General => "General",
            Coordinator => "Coordinator settings",
            Tools => "Tools",
            Skills => "Skills",
            Agents => "Agents",
            Mcp => "MCP servers",
            Hooks => "Hooks",
            MemorySettings => "O2 settings",
            SessionSettings => "Session settings",
            About => "About",
        }
    }
    pub fn mode(self) -> Mode {
        match self {
            Overview | Live | Workers | Inbox | Sessions | Memory | Logs | Doctor | About => {
                Mode::Operate
            }
            _ => Mode::Configure,
        }
    }
    pub fn open(self, s: &mut AppState) {
        s.inspector_index = 0;
        s.inspector_scroll = 0;
        let screen = match self {
            Overview => {
                s.home_activity = false;
                s.inspector_index = s
                    .observations
                    .latest
                    .as_ref()
                    .and_then(|o| crate::overview_view::next_step(&o.readiness))
                    .unwrap_or(0);
                Screen::Home
            }
            Live => Screen::CoordinatorLive,
            Workers => Screen::Watch,
            Inbox => Screen::Inbox,
            Logs => {
                s.refresh_logs();
                Screen::Logs
            }
            Doctor => {
                s.run_home_doctor_check();
                Screen::Doctor
            }
            Changes => Screen::Changes,
            Coordinator | General => {
                s.jump_config_tab(if self == General { 0 } else { 1 });
                Screen::Automation
            }
            Tools => Screen::Tools,
            Skills => Screen::Skills,
            Mcp => Screen::Mcp,
            Agents => Screen::Agents,
            About => Screen::About,
            Hooks => {
                if let Some(p) = &s.project_paths {
                    s.hooks.refresh(&p.root);
                }
                Screen::Hooks
            }
            Memory | MemorySettings | Sessions | SessionSettings => {
                let o2 = matches!(self, Memory | MemorySettings);
                session_navigation::open(s, o2, matches!(self, MemorySettings | SessionSettings));
                return;
            }
        };
        s.push_screen(screen);
    }
}
pub(crate) fn current(s: &AppState) -> Destination {
    match s.current_screen() {
        Screen::Home => Overview,
        Screen::CoordinatorLive => Live,
        Screen::Watch => Workers,
        Screen::Inbox => Inbox,
        Screen::Doctor => Doctor,
        Screen::Logs => Logs,
        Screen::Changes | Screen::Preview | Screen::Apply => Changes,
        Screen::Automation if s.config_tab_index != 0 => Coordinator,
        Screen::Automation => General,
        Screen::Tools | Screen::ToolSettings => Tools,
        Screen::Mcp => Mcp,
        Screen::Skills => Skills,
        Screen::Agents => Agents,
        Screen::Hooks => Hooks,
        Screen::Sessions if s.sessions.configuring() => SessionSettings,
        Screen::Sessions => Sessions,
        Screen::O2 if s.sessions.configuring() => MemorySettings,
        Screen::O2 => Memory,
        Screen::About => About,
    }
}
pub(crate) fn mode(s: &AppState) -> Mode {
    current(s).mode()
}
pub(crate) fn switch(s: &mut AppState) {
    let previous = current(s);
    let twin = match previous {
        Overview => Changes,
        Changes => Overview,
        Live | Workers => Coordinator,
        Coordinator if s.config_tab_index == 7 => Inbox,
        Coordinator => Live,
        Sessions => SessionSettings,
        SessionSettings => Sessions,
        Memory => MemorySettings,
        MemorySettings => Memory,
        Inbox => Coordinator,
        _ if mode(s) == Mode::Operate => Changes,
        _ => Overview,
    };
    twin.open(s);
    let n = s.screen_stack.len();
    if n >= 3 && s.screen_stack[n - 1] == s.screen_stack[n - 3] {
        s.screen_stack.truncate(n - 2);
    }
    if twin == Coordinator && previous == Inbox {
        s.jump_config_tab(7);
    }
}

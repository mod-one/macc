use super::*;
use crate::command_catalog::keys as k;
use crate::command_catalog::Command;
impl SessionsScreen {
    pub(crate) fn policy_changes(
        &self,
    ) -> impl Iterator<Item = (&'static str, Value, &Value, bool)> {
        let session = self.view.as_ref().and_then(|view| {
            let old = serde_json::to_value(&view.policy).ok()?;
            (self.inherit || old != self.draft).then_some((
                "session_management",
                old,
                &self.draft,
                self.inherit,
            ))
        });
        session.into_iter().chain(
            self.o2
                .policy_change()
                .map(|(old, new, inherit)| ("", old, new, inherit)),
        )
    }

    pub(crate) fn focus_policy_field(&mut self, path: &str) {
        if self.o2.open {
            self.o2.focus_policy_field(path);
        } else if let Some(index) = path.strip_prefix("session_management.").and_then(|key| {
            self.view
                .as_ref()?
                .fields
                .iter()
                .position(|f| f.path == key)
        }) {
            self.selected = index;
        }
    }

    pub fn configuring(&self) -> bool {
        if self.o2.open {
            self.o2.configure_section()
        } else {
            self.tab == 0 && !self.usage.open && !self.checkpoints.open
        }
    }
    pub fn open_o2(&mut self, root: &Path, configure: bool) -> macc_core::Result<()> {
        self.message.clear();
        self.usage.open = false;
        self.checkpoints.open = false;
        self.o2.open = true;
        self.o2.open_mode(root, configure)
    }
    pub fn draw_overlay(&self, f: &mut Frame) {
        if self.o2.open {
            self.o2.draw_overlay(f);
        }
    }
    pub fn show_sessions(&mut self, root: &Path, configure: bool) -> macc_core::Result<()> {
        self.o2.open = false;
        self.usage.open = false;
        self.checkpoints.open = false;
        self.tab = usize::from(!configure);
        self.selected = if configure {
            0
        } else {
            self.view.as_ref().map_or(0, |v| v.fields.len())
        };
        self.details.clear();
        self.scroll = 0;
        self.refresh(root)
    }
    pub(crate) fn restore_workspace(&mut self, o2: bool) {
        self.o2.open = o2;
    }
    pub fn o2_open(&self) -> bool {
        self.o2.open
    }
    pub fn input_active(&self) -> bool {
        if self.o2.open {
            self.o2.input_active()
        } else {
            self.editing.is_some()
        }
    }
    pub(crate) fn supports_raw(&self) -> bool {
        !self.usage.open && !self.checkpoints.open
    }
    pub(crate) fn toggle_raw(&mut self) {
        if self.o2.open {
            self.o2.toggle_raw();
        } else {
            self.raw_details = !self.raw_details;
        }
    }
    pub(crate) fn navigation_bindings(&self) -> Vec<(k::Keys, &'static str)> {
        if self.usage.open || self.checkpoints.open {
            return vec![
                (k::VERTICAL, "Select record"),
                (k::ENTER, "Inspect evidence"),
                (k::PAGES, "Scroll details"),
                (k::ESC, "Back"),
            ];
        }
        let mut bindings = vec![
            (k::VERTICAL, "Select"),
            (k::ENTER, "Edit / inspect"),
            (k::PAGES, "Scroll details"),
            (k::ESC, "Back"),
        ];
        if self.o2.open {
            bindings.extend([(k::TAB, "Next section"), (k::BACKTAB, "Previous section")]);
        }
        if self.o2.open && self.o2.has_horizontal_controls() {
            bindings.push((k::HORIZONTAL, "Fold or expand group / select trial field"));
        }
        bindings.extend(if self.o2.open {
            self.o2
                .sections()
                .iter()
                .enumerate()
                .map(|(i, (_, name))| (k::ch((b'1' + i as u8) as char), *name))
                .collect()
        } else {
            Vec::new()
        });
        bindings
    }
    pub(crate) fn commands(&self) -> Vec<Command> {
        if self.o2.open {
            return self.o2.commands();
        }
        if self.usage.open {
            return vec![
                Command::new('r', "Refresh usage", "Reload invocation receipts."),
                Command::new(
                    'u',
                    "Preview usage upgrade",
                    "Prepare a storage upgrade plan.",
                ),
                Command::new(
                    'a',
                    "Apply reviewed usage upgrade",
                    "Requires a preview; retains backup.",
                )
                .when(self.usage.has_reviewed_plan()),
                Command::new('x', "Discard upgrade", "Clear the pending upgrade plan."),
            ];
        }
        if self.checkpoints.open {
            return vec![
                Command::new('*', "Toggle all tasks", "Switch checkpoint scope."),
                Command::new(
                    'u',
                    "Preview checkpoint upgrade",
                    "Prepare a storage upgrade plan.",
                ),
                Command::new(
                    'p',
                    "Preview reconciliation",
                    "Inspect the selected checkpoint reconciliation.",
                ),
                Command::new(
                    'a',
                    "Apply reviewed operation",
                    "Apply the pending plan with its evidence.",
                )
                .when(self.checkpoints.has_reviewed_plan()),
                Command::new('x', "Discard operation", "Clear the pending plan."),
            ];
        }
        let sqlite = self.view.as_ref().is_some_and(|v| v.backend == "sqlite");
        let selected = self.selected_id().is_some();
        let mut commands = vec![
            Command::new(
                'a',
                "Confirm reviewed operation",
                "Apply the displayed storage plan or close the explicitly selected session.",
            )
            .when(self.plan.is_some() || self.closing.is_some()),
            Command::new(
                'r',
                "Refresh recorded sessions",
                "Refresh evidence while preserving unsaved settings.",
            ),
        ];
        commands.extend(if self.configuring() {vec![
            Command::new('p',"Preview session settings","Validate the current draft before saving; does not write settings."),
            Command::new('s',"Save session settings","Save the draft through the revision-checked policy service."),
            Command::new('m',if sqlite {"Prepare JSON rollback"}else{"Prepare SQLite migration"},"Prepare a storage plan. Inspect its evidence and stop active runners before confirming."),
            Command::new('d',"Stage default settings","Replace the draft with defaults; saving remains explicit."),
            Command::new('u',"Inherit default settings","Stage removal of project overrides; saving remains explicit."),
            Command::new('x',"Discard settings draft","Reload saved settings and clear pending storage/session confirmations."),
        ]} else {vec![
            Command::new('o',"Open O2 memory & context","Inspect shared memory, context, compaction and code intelligence."),
            Command::new('t',"Inspect token usage","Browse captured usage and its measurement quality."),
            Command::new('h',"Inspect checkpoints","View task evidence and reviewed reconciliation plans."),
            Command::new('c',"Prepare session closure","Select a conversation first; active leased sessions cannot close.").when(selected),
            Command::new('v',"Recover dead-owner leases","Quarantine expired leases only when their recorded owner is dead.").when(sqlite),
            Command::new('b',"Create consistent backup","Back up the current authority and retain the resulting evidence path.").when(sqlite),
        ]});
        commands
    }
    pub fn section_name(&self) -> &str {
        if self.o2.open {
            self.o2.section_name()
        } else if self.usage.open {
            "Token usage"
        } else if self.checkpoints.open {
            "Checkpoints"
        } else if self.tab == 0 {
            "Session settings"
        } else {
            "Conversations"
        }
    }
}

use super::*;
use crate::workspace_ui::Command;
impl SessionsScreen {
    pub fn open_o2(&mut self, root: &Path) -> macc_core::Result<()> {
        if self.o2.view_is_missing() {
            self.o2.dashboard();
        }
        self.message.clear();
        self.o2.refresh(root)?;
        self.o2.open = true;
        Ok(())
    }
    pub fn draw_overlay(&self, f: &mut Frame) {
        if self.o2.open {
            self.o2.draw_overlay(f);
        }
    }
    pub fn show_sessions(&mut self) {
        self.o2.open = false;
        self.usage.open = false;
        self.checkpoints.open = false;
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
    pub(crate) fn navigation_bindings(&self) -> Vec<(&'static str, &'static str)> {
        if self.usage.open || self.checkpoints.open {
            return vec![
                ("↑↓", "Select record"),
                ("Enter", "Inspect evidence"),
                ("PgUp/PgDn", "Scroll details"),
                ("Esc", "Back"),
            ];
        }
        let mut bindings = vec![
            ("Tab", "Next section"),
            ("Shift+Tab", "Previous section"),
            ("↑↓", "Select"),
            ("Enter", "Edit / inspect"),
            ("PgUp/PgDn", "Scroll details"),
            ("F4", "Raw evidence"),
            ("O", "O2 workspace"),
            ("J", "Sessions"),
            ("Esc", "Back"),
        ];
        if self.o2.open && self.o2.has_horizontal_controls() {
            bindings.push(("←→", "Setting group / trial field"));
        }
        bindings.extend(if self.o2.open {
            vec![
                ("1", "Memory"),
                ("2", "Context"),
                ("3", "Compaction"),
                ("4", "Code"),
                ("5", "Settings"),
                ("6", "Overview"),
                ("7", "Trials"),
            ]
        } else {
            vec![("1", "Settings"), ("2", "Conversations")]
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
        vec![
            Command::new('o',"Open O2 memory & context","Inspect shared memory, context, compaction and code intelligence."),
            Command::new('t',"Inspect token usage","Browse captured usage and its measurement quality."),
            Command::new('h',"Inspect checkpoints","View task evidence and reviewed reconciliation plans."),
            Command::new('p',"Preview session settings","Validate the current draft before saving; does not write settings."),
            Command::new('s',"Save session settings","Save the draft through the revision-checked policy service."),
            Command::new('m',if sqlite {"Prepare JSON rollback"}else{"Prepare SQLite migration"},"Prepare a storage plan. Inspect its evidence and stop active runners before confirming."),
            Command::new('a',"Confirm reviewed operation","Apply the displayed storage plan or close the explicitly selected session.").when(self.plan.is_some() || self.closing.is_some()),
            Command::new('c',"Prepare session closure","Select a conversation first; active leased sessions cannot close.").when(selected),
            Command::new('v',"Recover dead-owner leases","Quarantine expired leases only when their recorded owner is dead.").when(sqlite),
            Command::new('b',"Create consistent backup","Back up the current authority and retain the resulting evidence path.").when(sqlite),
            Command::new('d',"Stage default settings","Replace the draft with defaults; saving remains explicit."),
            Command::new('u',"Inherit default settings","Stage removal of project overrides; saving remains explicit."),
            Command::new('x',"Discard settings draft","Reload saved settings and clear pending storage/session confirmations."),
            Command::new('r',"Refresh recorded sessions","Refresh evidence while preserving unsaved settings."),
        ]
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

//! Contextual commands are shared by the action picker and help.
use super::*;
use crate::command_catalog::Command;
impl Panel {
    pub(crate) fn dirty(&self) -> bool {
        self.policy_change().is_some()
    }
    pub(crate) fn policy_change(&self) -> Option<(Value, &Value, bool)> {
        let old = serde_json::to_value(&self.view.as_ref()?.policy).ok()?;
        (self.inherit || old != self.draft).then_some((old, &self.draft, self.inherit))
    }

    pub(crate) fn focus_policy_field(&mut self, path: &str) {
        if let Some(index) = self
            .view
            .as_ref()
            .and_then(|v| v.fields.iter().position(|f| f.path == path))
        {
            self.selected = index;
            if let Some(group) = self.selected_group() {
                self.folded.remove(&group);
            }
        }
    }
    pub(crate) fn is_boolean_setting(&self) -> bool {
        self.tab == 4
            && self
                .view
                .as_ref()
                .and_then(|v| v.fields.get(self.selected))
                .is_some_and(|f| f.kind == "bool")
    }
    pub(crate) fn configure_section(&self) -> bool {
        matches!(self.tab, 2..=4)
    }

    pub(crate) fn has_horizontal_controls(&self) -> bool {
        self.tab == 6 || self.tab == 4 && self.selected_group().is_some()
    }

    pub fn input_active(&self) -> bool {
        self.form.is_some()
    }
    pub fn section_name(&self) -> &'static str {
        [
            "Memory",
            "Context",
            "Compaction",
            "Code intelligence",
            "Settings",
            "Overview",
            "Policy trials",
        ][self.tab]
    }
    pub fn commands(&self) -> Vec<Command> {
        let ready = self.view.as_ref().is_some_and(|v| v.ready);
        let selected = !self.rows.is_empty();
        let mut commands = vec![Command::new(
            'r',
            "Refresh this section",
            "Refresh the current scope while retaining unsaved settings.",
        )];
        if self.tab == 4 {
            commands.push(Command::new(
                '[',
                "Previous policy category",
                "Memory, context, compaction or code intelligence.",
            ));
            commands.push(Command::new(
                ']',
                "Next policy category",
                "Memory, context, compaction or code intelligence.",
            ));
        }
        if self.is_boolean_setting()
            && !self
                .selected_group()
                .is_some_and(|g| self.folded.contains(&g))
        {
            commands.push(Command::new(
                ' ',
                "Toggle setting",
                "Change the draft. Preview and save to persist.",
            ));
        }
        let mut add =
            |key, label, hint, enabled| commands.push(Command::new(key, label, hint).when(enabled));
        match self.tab {
            0 => {
                add(
                    '/',
                    "Search / change memory scope",
                    "Choose task, feature, tool, user or worktree scope explicitly.",
                    ready,
                );
                add(
                    'n',
                    "Propose a sourced memory",
                    "Guided proposal form; newly proposed facts are not automatically trusted.",
                    ready,
                );
                add(
                    'p',
                    "Review selected memory",
                    "Inspect evidence and prepare a transition before confirming.",
                    ready && selected && !self.conflicts,
                );
                add(
                    'E',
                    "Export reviewed memories",
                    "Select records and a new output file; preview and confirm explicitly.",
                    ready,
                );
                add(
                    'I',
                    "Import memories as proposals",
                    "Choose a JSONL or MACC Markdown document; preserve local reviews.",
                    ready,
                );
                add(
                    'f',
                    "Switch inbox / conflict list",
                    "Inspect contradictory claims without changing their review state.",
                    ready,
                );
                add(
                    'c',
                    "Record a conflict",
                    "Choose another fact after inspecting the selected claim.",
                    ready && selected && !self.conflicts,
                );
                add(
                    'v',
                    "Resolve selected conflict",
                    "Prepare a conservative reviewed outcome; no automatic promotion.",
                    ready && selected && self.conflicts,
                );
                add(
                    'y',
                    "Capture source-symbol evidence",
                    "Locate a current symbol through the registered code provider.",
                    ready,
                );
                add(
                    'Y',
                    "Capture tool-version evidence",
                    "Capture a registered tool's observed identity for later review.",
                    ready,
                );
                add(
                    'R',
                    "Review with captured evidence",
                    "Use the freshly captured evidence for the selected fact.",
                    ready && selected && self.captured_evidence.is_some(),
                );
                add(
                    'h',
                    "Inspect agent memory access",
                    "Inspect scoped MCP setup and available read/proposal capabilities.",
                    ready,
                );
                add(
                    'e',
                    "Extract checkpoint proposals",
                    "Prepare proposals from a verified finished checkpoint; never activate facts.",
                    ready,
                );
                add(
                    'Q',
                    "Search approved memory",
                    "Use the current approved semantic/lexical retrieval policy.",
                    ready,
                );
                add(
                    'S',
                    "Inspect semantic benchmark status",
                    "Inspect benchmark and adoption evidence.",
                    ready,
                );
                add(
                    'B',
                    "Prepare semantic benchmark",
                    "Enter representative labelled queries and inspect measured results.",
                    ready,
                );
                add(
                    'G',
                    "Prepare semantic approval",
                    "Requires an exact passing benchmark for the current scope.",
                    ready && self.semantic_report.is_some(),
                );
                add(
                    'X',
                    "Prepare semantic revocation",
                    "Revoke approval only after inspecting its exact report.",
                    ready && self.semantic_report.is_some(),
                );
                add(
                    'l',
                    "Inspect agent search page",
                    "Inspect bounded search output exposed to agents.",
                    ready,
                );
                add(
                    'g',
                    "Inspect agent record fragment",
                    "Inspect a bounded fragment of the selected fact.",
                    ready && selected,
                );
                add(
                    'b',
                    "Rebuild memory search index",
                    "Rebuild derived lexical search data without changing memory reviews.",
                    ready,
                );
            }
            1 => add(
                'b',
                "Reduce captured tool output",
                "Preserve the raw artifact and prepare an explicitly bounded summary.",
                ready,
            ),
            2 => {
                add('p',"Probe selected tool capabilities","Run declared version/help probes; native compression still requires observations.",ready&&selected);
                add(
                    'd',
                    "Inspect runtime decisions",
                    "Inspect compaction state and adapter capability evidence.",
                    ready,
                );
                add(
                    '/',
                    "Simulate compaction decision",
                    "Enter observed usage and budgets without running a model.",
                    ready,
                );
                add(
                    'c',
                    "Prepare a checkpoint",
                    "Choose an idle logical session through the shared authority.",
                    ready,
                );
                add('m',"Preview native-setting adoption","Adopt an idle session's existing setting only after a reviewed matching probe.",ready);
                add(
                    'v',
                    "Prepare session rotation",
                    "Choose an exact idle logical session; inspect before confirmation.",
                    ready,
                );
            }
            3 => {
                add(
                    '/',
                    "Query current code",
                    "Choose a query kind and explicit worktree/path through provider adapters.",
                    ready,
                );
                add('p',"Prepare provider lifecycle action","Choose install, deploy, index, update, rollback or removal. Nothing runs until confirmed.",ready);
                add('v',"Compare provider with native","Inspect the same query through the selected registered provider and native fallback.",ready);
                add(
                    'd',
                    "Inspect provider operation",
                    "Inspect a recorded lifecycle plan/operation UUID.",
                    ready,
                );
            }
            4 => {
                add(
                    '/',
                    "Find a setting",
                    "Search all groups by canonical name or friendly label.",
                    true,
                );
                add(
                    'p',
                    "Preview",
                    "Validate the complete current draft without saving.",
                    true,
                );
                add(
                    's',
                    "Save",
                    "Requires a successful preview of the exact current revision.",
                    self.preview,
                );
                add(
                    'x',
                    "Discard",
                    "Reload saved policy values without changing active tasks.",
                    true,
                );
                add(
                    'b',
                    "Stage default settings",
                    "Replace the draft with defaults; preview/save remain explicit.",
                    true,
                );
                add(
                    'i',
                    "Stage inherited settings",
                    "Prepare removal of project overrides; preview/save remain explicit.",
                    true,
                );
            }
            5 => {
                add(
                    '/',
                    "Filter task overview",
                    "Enter an exact task identifier; blank shows all tasks.",
                    ready,
                );
                add(
                    'm',
                    "Inspect selected task's memory",
                    "Open memory scoped to this task, preserving policy drafts.",
                    ready && selected,
                );
                add(
                    't',
                    "Inspect selected task's context",
                    "Open context manifests for this exact task.",
                    ready && selected,
                );
                add(
                    's',
                    "Inspect selected task's sessions",
                    "Open sessions/checkpoints for this exact task.",
                    ready && selected,
                );
                add(
                    '[',
                    "First task page",
                    "Return to the beginning of the bounded overview.",
                    ready,
                );
                add(
                    ']',
                    "Next task page",
                    "Follow the recorded next-page cursor when available.",
                    ready,
                );
            }
            _ => {
                add(
                    'n',
                    "Pre-register a policy trial",
                    "Choose representative paired tasks and an exact safe policy candidate.",
                    ready,
                );
                add(
                    'i',
                    "Inspect a trial",
                    "Load retained measurements and quality decisions.",
                    ready,
                );
                add(
                    'q',
                    "Record paired quality review",
                    "Attest quality separately from measured costs.",
                    ready,
                );
                add(
                    'e',
                    "Evaluate a trial",
                    "Check the pre-registered quality and measured benefit gates.",
                    ready,
                );
                add(
                    'p',
                    "Preview a passing report",
                    "Inspect the exact candidate before preparing adoption.",
                    ready,
                );
                add(
                    's',
                    "Prepare policy adoption",
                    "Requires an exact passing, inspected report; confirmation remains separate.",
                    ready,
                );
                add(
                    'x',
                    "Cancel a trial",
                    "Prepare explicit cancellation of the selected trial.",
                    ready,
                );
            }
        }
        add(
            'u',
            "Prepare O2 schema upgrade",
            "Requires SQLite. Inspect the source hash and backup conditions before confirming.",
            self.view.as_ref().is_some_and(|v| v.sqlite),
        );
        add(
            'a',
            "Confirm reviewed operation",
            "Apply only the exact operation displayed in the review pane.",
            self.pending.is_some() && self.job.is_none(),
        );
        add(
            'j',
            "Advanced typed operation",
            "Optional expert JSON interface; normal operations use the guided actions above.",
            true,
        );
        commands
    }
}

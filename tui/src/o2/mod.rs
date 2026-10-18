//! Interactive O2 clients call the same application service as CLI and Web.
mod commands;
mod forms;
mod jobs;
mod overview;
mod semantic;
mod semantic_forms;
#[cfg(test)]
mod semantic_tests;
mod settings;
mod transfer;
#[cfg(test)]
mod transfer_tests;
mod tuning;
#[cfg(test)]
mod tuning_tests;
mod view;
use crossterm::event::KeyCode;
use macc_core::{
    o2::{
        actions::{execute, Action},
        policy,
    },
    MaccError, Result,
};
use ratatui::{layout::Rect, Frame};
use serde_json::{json, Value};
use std::path::Path;
#[derive(Default)]
pub struct Panel {
    pub open: bool,
    menu: crate::workspace_ui::ActionMenu,
    raw_details: bool,
    pub session_task: Option<String>,
    overview: overview::Overview,
    tuning_field: usize,
    transfer: transfer::Transfer,
    tuning_preview: Option<Value>,
    tab: usize,
    selected: usize,
    scroll: u16,
    view: Option<policy::View>,
    draft: Value,
    revision: String,
    inherit: bool,
    preview: bool,
    rows: Vec<Value>,
    details: String,
    message: String,
    form: Option<forms::Form>,
    pending: Option<Action>,
    access: Value,
    query: String,
    conflicts: bool,
    semantic_report: Option<Value>,
    semantic_scope: Value,
    captured_evidence: Option<Value>,
    captured_scope: Value,
    job: Option<jobs::Job>,
}
impl Panel {
    pub fn mutation(&self, key: KeyCode) -> bool {
        if self.tab == 6 && key == KeyCode::Char('s') {
            return false;
        }
        if self.tab == 5 && key == KeyCode::Char('s') {
            return false;
        }
        if let Some(f) = &self.form {
            return key == KeyCode::Enter && f.mutation;
        }
        matches!(key, KeyCode::Char('a' | 's' | 'n' | 'v' | 'c'))
            || [2, 3].contains(&self.tab) && key == KeyCode::Char('p')
            || self.tab == 0 && key == KeyCode::Char('b')
    }
    pub fn refresh(&mut self, root: &Path) -> Result<()> {
        if self.access.is_null() {
            self.access = json!({});
        }
        if self.captured_scope != self.access {
            self.captured_evidence = None;
        }
        if self.semantic_scope != self.access {
            self.semantic_report = None;
        }
        let view = policy::inspect(root)?;
        if self.view.is_none() {
            self.revision = view.revision.clone();
            self.draft = serde_json::to_value(&view.policy)
                .map_err(|e| MaccError::Validation(e.to_string()))?;
        }
        self.view = Some(view);
        if self.tab == 5 {
            if !self.view.as_ref().is_some_and(|v| v.ready) {
                self.rows.clear();
                return Ok(());
            }
            return self.refresh_overview(root);
        }
        if self.tab == 6 {
            if !self.view.as_ref().is_some_and(|v| v.ready) {
                self.rows.clear();
                return Ok(());
            }
            if self.job.is_none() {
                self.launch(root, Action::TuningStatus { id: None })?;
            }
            return Ok(());
        }
        let ready = self.view.as_ref().is_some_and(|v| v.ready);
        if ready {
            let action = match self.tab {
                0 if self.conflicts => Some(Action::MemoryConflicts),
                0 => Some(Action::MemorySearch {
                    search: serde_json::from_value(
                        json!({"query":self.query,"access":self.access,"include_inactive":true}),
                    )
                    .unwrap(),
                }),
                1 => Some(Action::ContextList),
                2 => Some(Action::CompactionList),
                3 => Some(Action::Providers),
                _ => None,
            };
            if let Some(action) = action {
                if matches!(&action, Action::Providers | Action::MemorySearch { .. }) {
                    if self.job.is_none() {
                        self.launch(root, action)?;
                    }
                    return Ok(());
                }
                let data = execute(root, action)?;
                let rows = if self.tab == 3 {
                    data["providers"].as_array().cloned().unwrap_or_default()
                } else {
                    data.as_array().cloned().unwrap_or_default()
                };
                let rows = if self.tab == 1 {
                    rows.into_iter()
                        .filter(|r: &Value| {
                            self.access["task_id"]
                                .as_str()
                                .is_none_or(|t| r["task_id"] == t)
                        })
                        .collect()
                } else {
                    rows
                };
                self.replace_rows(rows);
            }
        }
        let count = if self.tab == 4 {
            self.view
                .as_ref()
                .map(|v| v.fields.len())
                .unwrap_or_default()
        } else {
            self.rows.len()
        };
        self.selected = self.selected.min(count.saturating_sub(1));
        Ok(())
    }
    fn result(&mut self, value: Value) {
        if self.tab == 6 {
            self.tuning_preview = (value["report_id"].is_string() && value["before"].is_object())
                .then(|| value.clone());
            if self.tuning_preview.is_none()
                && matches!(self.pending, Some(Action::TuningApply { .. }))
            {
                self.pending = None;
            }
        }
        if self.tab == 0 {
            self.semantic_result(&value);
        }
        if value["binding"].is_object()
            && (value["kind"] == "symbol" || value["kind"] == "provider")
        {
            self.captured_evidence = Some(value.clone());
            self.captured_scope = self.access.clone();
        }
        self.details = serde_json::to_string_pretty(&value).unwrap_or_default();
        self.scroll = 0;
    }
    fn selected(&self) -> Result<&Value> {
        self.rows
            .get(self.selected)
            .ok_or_else(|| MaccError::Validation("Select a record first".into()))
    }
    pub fn key(&mut self, root: &Path, key: KeyCode) -> Result<()> {
        if self.form.is_some() {
            return self.form_key(root, key);
        }
        if key == KeyCode::Esc && self.pending.is_some() {
            self.pending = None;
            self.details.clear();
            self.message = "Reviewed operation discarded; no action was applied.".into();
            return Ok(());
        }
        if key == KeyCode::F(4) {
            self.raw_details = !self.raw_details;
            return Ok(());
        }
        if matches!(key, KeyCode::Tab | KeyCode::BackTab) {
            let next = if key == KeyCode::Tab {
                (self.tab + 1) % 7
            } else {
                (self.tab + 6) % 7
            };
            return self.key(root, KeyCode::Char((b'1' + next as u8) as char));
        }
        if let KeyCode::Char(c @ '1'..='7') = key {
            if self.tab == 5 {
                self.overview.selected_task = self
                    .rows
                    .get(self.selected)
                    .and_then(|v| v["task_id"].as_str().map(str::to_string));
            }
            self.tuning_preview = None;
            if self.pending.as_ref().is_some_and(|a| {
                matches!(
                    a,
                    Action::TuningCreate { .. }
                        | Action::TuningObserve { .. }
                        | Action::TuningClose { .. }
                        | Action::TuningApply { .. }
                )
            }) {
                self.pending = None;
            }
            self.rows.clear();
            self.tab = c as usize - '1' as usize;
            self.selected = 0;
            self.scroll = 0;
            self.details.clear();
            if self.tab == 4 {
                self.selected = self
                    .view
                    .as_ref()
                    .and_then(|v| v.fields.iter().position(|f| f.path == "memory.enabled"))
                    .unwrap_or(0);
            }
            if self.tab == 5 {
                self.rows = self
                    .overview
                    .snapshot
                    .as_ref()
                    .and_then(|v| v["tasks"].as_array().cloned())
                    .unwrap_or_default();
                self.selected = self
                    .rows
                    .iter()
                    .position(|v| v["task_id"].as_str() == self.overview.selected_task.as_deref())
                    .unwrap_or(0);
            }
            self.refresh(root)?;
            return Ok(());
        }
        if self.overview_key(root, key)? {
            return Ok(());
        }
        if self.tuning_key(root, key)? {
            return Ok(());
        }
        if self.semantic_key(root, key)? {
            return Ok(());
        }
        if self.tab == 4 && self.setting_key(root, key)? {
            return Ok(());
        }
        if self.transfer_key(root, key)? {
            return Ok(());
        }
        let previous_selection = self.selected;
        match key {
            KeyCode::Esc|KeyCode::Char('o')=>{if self.view.as_ref().is_some_and(|v|self.inherit||serde_json::to_value(&v.policy).ok().as_ref()!=Some(&self.draft)){self.message="Unsaved policy draft. Use 5 → x to discard, or p then s to apply.".into();}else{self.open=false;}},
            KeyCode::Char('r')=>self.refresh(root)?,KeyCode::Up=>self.selected=self.selected.saturating_sub(1),KeyCode::Down=>self.selected=(self.selected+1).min(self.rows.len().saturating_sub(1)),KeyCode::PageDown=>self.scroll=self.scroll.saturating_add(10),KeyCode::PageUp=>self.scroll=self.scroll.saturating_sub(10),
            KeyCode::Enter=>{let mut row=self.selected()?.clone();if self.tab==0 && !self.conflicts {row=execute(root,Action::MemoryGet{id:row["memory"]["id"].as_str().unwrap_or_default().into(),access:serde_json::from_value(self.access.clone()).map_err(|e|MaccError::Validation(e.to_string()))?})?;self.rows[self.selected]=row.clone();}self.result(row);},
            KeyCode::Char('d') if self.tab==2=>{let data=execute(root,Action::CompactionState)?;self.result(data);},
            KeyCode::Char('m') if self.tab==2=>self.start("adopt_native",true,&["Idle logical session ID whose existing native setting should become MACC-managed"]),
            KeyCode::Char('c') if self.tab==2=>self.start("checkpoint",true,&["Idle logical session ID for checkpoint"]),
            KeyCode::Char('u')=>{let plan=execute(root,Action::UpgradePlan)?;self.pending=Some(Action::UpgradeApply{source_sha256:plan["source_sha256"].as_str().unwrap_or_default().into()});self.result(plan);self.message="Inspect the upgrade and backup conditions. Press a to apply this exact reviewed source hash.".into();},
            KeyCode::Char('a')=>{if let Some(action)=self.pending.as_ref(){if matches!(action,Action::TuningCreate{..}|Action::TuningObserve{..}|Action::TuningClose{..}|Action::TuningApply{..}|Action::ProviderApply{..}|Action::SemanticBenchmark{..}|Action::SemanticGate{..}|Action::MemoryReview{..}|Action::MemoryResolveConflict{..}|Action::MemoryToolEvidence{..}){self.launch(root,action.clone())?;return Ok(());}let result=execute(root,action.clone())?;self.pending=None;self.result(result);self.refresh(root)?;self.message="Reviewed operation applied.".into();}},
            KeyCode::Char('h') if self.tab==0=>{let data=execute(root,Action::AgentStatus)?;self.result(data);},
            KeyCode::Char('e') if self.tab==0=>self.start("extract",true,&["Verified finished checkpoint ID to extract proposals (no memory activation)"]),
            KeyCode::Char('y') if self.tab==0=>self.start("capture_symbol",false,&["Tracked Rust source path","Qualified symbol (module/Type/method)","Provider ID (native)","Worktree (blank for project)"]),
            KeyCode::Char('Y') if self.tab==0=>self.start("capture_tool",true,&["Registered tool ID (see evidence sources via j: memory_evidence_sources)"]),
            KeyCode::Char('n') if self.tab==0=>self.start("propose",true,&["Kind (discovery, constraint, decision…)","Title","Body (concise sourced fact)","Scope (project, task, feature, worktree, tool, user)","Scope reference (blank for project)","Evidence source path (blank uses captured bound evidence)","Confidence (0..1)"]),
            KeyCode::Char('R') if self.tab==0 && !self.conflicts=>{self.selected()?;if self.captured_evidence.is_none(){return Err(MaccError::Validation("Capture fresh evidence with y/Y first".into()));}self.start("review_captured",true,&["Transition (active for revalidation)","Reason after reviewing the newly captured evidence"]);},
            KeyCode::Char('p') if self.tab==0 && !self.conflicts=>{self.selected()?;self.start("review",true,&["Transition (active, rejected, stale, invalidated)","Review reason","Replacement evidence path (blank preserves existing evidence)"]);},
            KeyCode::Char('c') if self.tab==0 && !self.conflicts=>{self.selected()?;self.start("conflict",true,&["Conflicting memory ID","Reason after inspecting both claims"]);},
            KeyCode::Char('p') if self.tab==3=>self.start("provider",true,&["Provider ID (native, graphify, serena)","Operation (install, deploy, index, update, rollback, disable, undeploy, uninstall)"]),
            KeyCode::Char('p') if self.tab==2=>{let tool=self.selected()?["tool_id"].as_str().unwrap_or_default().to_string();let data=execute(root,Action::CompactionProbe{tool_id:tool})?;self.result(data);self.refresh(root)?;},
            KeyCode::Char('v') if self.tab==3=>self.start("compare",false,&["Candidate provider ID (graphify or serena)","Query text","Kind (file_search, references, symbol_definition, architecture_summary)","Worktree (blank for project)","Definition file path (required for Serena)"]),
            KeyCode::Char('d') if self.tab==3=>self.start("operation",false,&["Provider plan/operation UUID"]),
            KeyCode::Char('v') if self.tab==2=>self.start("rotate",true,&["Exact logical session ID to close (inspect Sessions first)"]),
            KeyCode::Char('/')=>match self.tab{0=>self.start("search",false,&["Search text (blank lists inbox)","Task scope (blank for project facts)","Tool scope","Worktree scope","Feature scope","User scope"]),2=>self.start("decision",false,&["Simulated uses","Invocation budget","Observed context tokens (blank unknown)","Effective window (blank unknown)"]),3=>self.start("code",false,&["Literal code query","Query kind (file_search, references, symbol_definition, architecture_summary)","Worktree (blank for project)","Definition file path (required for Serena)"]),_=>{}},
            KeyCode::Char('b') if self.tab==0=>{let result=execute(root,Action::FtsRebuild)?;self.result(result);},
            KeyCode::Char('f') if self.tab == 0 => {
                self.conflicts = !self.conflicts;
                self.selected = 0;
                self.refresh(root)?;
            }
            KeyCode::Char('v') if self.tab==0 && self.conflicts=>{self.selected()?;self.start("resolve_conflict",true,&["Reason after reviewing both conflicting claims","Outcome (both_stale, keep_left, keep_right, dismiss; blank marks active claims stale)"]);},
            KeyCode::Char('l') if self.tab==0=>self.start("pull",false,&["Agent search cursor (0 for first page)"]),
            KeyCode::Char('g') if self.tab==0 && !self.conflicts=>{self.selected()?;self.start("read",false,&["Agent record offset (0 for first fragment)"]);},
            KeyCode::Char('j')=>self.start("advanced",false,&["Advanced typed operation JSON (all shared O2 actions)"]),
            KeyCode::Char('b') if self.tab==1=>self.start("reduce",true,&["Reduction bundle (test, lint, trace, diff, log)","Raw output (JSON escapes supported through advanced j)"]),
            _=>{}
        }
        if self.selected != previous_selection && self.pending.is_none() {
            self.details.clear();
            self.scroll = 0;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

//! Interactive O2 clients call the same application service as CLI and Web.
mod forms;
mod settings;
use crossterm::event::KeyCode;
use macc_core::{
    o2::{
        actions::{execute, Action},
        policy,
    },
    MaccError, Result,
};
use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use serde_json::{json, Value};
use std::path::Path;
#[derive(Default)]
pub struct Panel {
    pub open: bool,
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
}
impl Panel {
    pub fn mutation(&self, key: KeyCode) -> bool {
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
        let view = policy::inspect(root)?;
        if self.view.is_none() {
            self.revision = view.revision.clone();
            self.draft = serde_json::to_value(&view.policy)
                .map_err(|e| MaccError::Validation(e.to_string()))?;
        }
        self.view = Some(view);
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
                let data = execute(root, action)?;
                self.rows = if self.tab == 3 {
                    data["providers"].as_array().cloned().unwrap_or_default()
                } else {
                    data.as_array().cloned().unwrap_or_default()
                };
            }
        }
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        Ok(())
    }
    fn result(&mut self, value: Value) {
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
        if let KeyCode::Char(c @ '1'..='5') = key {
            self.tab = c as usize - '1' as usize;
            self.selected = 0;
            self.scroll = 0;
            self.refresh(root)?;
            return Ok(());
        }
        if self.tab == 4 && self.setting_key(root, key)? {
            return Ok(());
        }
        match key {
            KeyCode::Esc|KeyCode::Char('o')=>{if self.view.as_ref().is_some_and(|v|self.inherit||serde_json::to_value(&v.policy).ok().as_ref()!=Some(&self.draft)){self.message="Unsaved policy draft. Use 5 → x to discard, or p then s to apply.".into();}else{self.open=false;}},
            KeyCode::Char('r')=>self.refresh(root)?,KeyCode::Up=>self.selected=self.selected.saturating_sub(1),KeyCode::Down=>self.selected=(self.selected+1).min(self.rows.len().saturating_sub(1)),KeyCode::PageDown=>self.scroll=self.scroll.saturating_add(10),KeyCode::PageUp=>self.scroll=self.scroll.saturating_sub(10),
            KeyCode::Enter=>{let mut row=self.selected()?.clone();if self.tab==0 && !self.conflicts {row=execute(root,Action::MemoryGet{id:row["memory"]["id"].as_str().unwrap_or_default().into(),access:serde_json::from_value(self.access.clone()).map_err(|e|MaccError::Validation(e.to_string()))?})?;self.rows[self.selected]=row.clone();}self.result(row);},
            KeyCode::Char('d') if self.tab==2=>{let data=execute(root,Action::CompactionState)?;self.result(data);},
            KeyCode::Char('m') if self.tab==2=>self.start("adopt_native",true,&["Idle logical session ID whose existing native setting should become MACC-managed"]),
            KeyCode::Char('c') if self.tab==2=>self.start("checkpoint",true,&["Idle logical session ID for checkpoint"]),
            KeyCode::Char('u')=>{let plan=execute(root,Action::UpgradePlan)?;self.pending=Some(Action::UpgradeApply{source_sha256:plan["source_sha256"].as_str().unwrap_or_default().into()});self.result(plan);self.message="Inspect the upgrade and backup conditions. Press a to apply this exact reviewed source hash.".into();},
            KeyCode::Char('a')=>{if let Some(action)=self.pending.as_ref(){let result=execute(root,action.clone())?;self.pending=None;self.result(result);self.refresh(root)?;self.message="Reviewed operation applied.".into();}},
            KeyCode::Char('n') if self.tab==0=>self.start("propose",true,&["Kind (discovery, constraint, decision…)","Title","Body (concise sourced fact)","Scope (project, task, feature, worktree, tool, user)","Scope reference (blank for project)","Evidence source path","Confidence (0..1)"]),
            KeyCode::Char('p') if self.tab==0 && !self.conflicts=>{self.selected()?;self.start("review",true,&["Transition (active, rejected, stale, invalidated)","Review reason","Replacement evidence path (blank preserves existing evidence)"]);},
            KeyCode::Char('c') if self.tab==0 && !self.conflicts=>{self.selected()?;self.start("conflict",true,&["Conflicting memory ID","Reason after inspecting both claims"]);},
            KeyCode::Char('p') if self.tab==3=>self.start("provider",true,&["Provider ID (native)","Operation (install, deploy, index, update, rollback, disable, undeploy, uninstall)"]),
            KeyCode::Char('p') if self.tab==2=>{let tool=self.selected()?["tool_id"].as_str().unwrap_or_default().to_string();let data=execute(root,Action::CompactionProbe{tool_id:tool})?;self.result(data);self.refresh(root)?;},
            KeyCode::Char('v') if self.tab==2=>self.start("rotate",true,&["Exact logical session ID to close (inspect Sessions first)"]),
            KeyCode::Char('/')=>match self.tab{0=>self.start("search",false,&["Search text (blank lists inbox)","Task scope (blank for project facts)","Tool scope","Worktree scope","Feature scope","User scope"]),2=>self.start("decision",false,&["Simulated uses","Invocation budget","Observed context tokens (blank unknown)","Effective window (blank unknown)"]),3=>self.start("code",false,&["Literal code query","Query kind (file_search or references)","Worktree (blank for project)"]),_=>{}},
            KeyCode::Char('b') if self.tab==0=>{let result=execute(root,Action::FtsRebuild)?;self.result(result);},
            KeyCode::Char('f') if self.tab == 0 => {
                self.conflicts = !self.conflicts;
                self.selected = 0;
                self.refresh(root)?;
            }
            KeyCode::Char('v') if self.tab==0 && self.conflicts=>{self.selected()?;self.start("resolve_conflict",true,&["Reason after reviewing both conflicting claims"]);},
            KeyCode::Char('l') if self.tab==0=>self.start("pull",false,&["Agent search cursor (0 for first page)"]),
            KeyCode::Char('g') if self.tab==0 && !self.conflicts=>{self.selected()?;self.start("read",false,&["Agent record offset (0 for first fragment)"]);},
            KeyCode::Char('j')=>self.start("advanced",false,&["Advanced typed operation JSON (all shared O2 actions)"]),
            KeyCode::Char('b') if self.tab==1=>self.start("reduce",true,&["Reduction bundle (test, lint, trace, diff, log)","Raw output (JSON escapes supported through advanced j)"]),
            _=>{}
        }
        Ok(())
    }
    pub fn draw(&self, f: &mut Frame, area: Rect, outer_message: &str) {
        let mut lines=vec!["1 Memory | 2 Context | 3 Compaction | 4 Code Intelligence | 5 Policies | r refresh | u preview schema upgrade | a apply reviewed action | j advanced action | o/Esc back".into(),"↑↓ select | Enter inspect | PgUp/PgDn scroll | / scoped search or decision simulation".into(),format!("SQLite: {} | O2 ready: {}",self.view.as_ref().is_some_and(|v|v.sqlite),self.view.as_ref().is_some_and(|v|v.ready)),self.message.clone()];
        lines.push(outer_message.into());
        if self.tab == 4 {
            self.draw_settings(&mut lines);
        } else {
            lines.push(match self.tab{0=>"n propose | p review → a confirm | c conflict | f toggle conflict list | v resolve selected conflict | l agent search page | g agent record fragment | b rebuild FTS. Only reviewed active current facts enter prompts.",1=>"Context manifests keep PRD/criteria and pinned source hashes separate from optional estimated memory. b reduce output and preserve raw artifact.",2=>"p probe version/help | d runtime decisions | c create checkpoint | m preview native setting adoption | v review scoped rotation. Native settings are opt-in under Policies. Compression execution remains unobserved unless an adapter reports it.",_=>"p preview lifecycle → a confirm. Native ships with MACC; external adapters are optional. / literal current-code query."}.into());
            for (i, row) in self.rows.iter().enumerate() {
                let label = if self.tab == 0 && self.conflicts {
                    format!("{} | {} | {}", row["id"], row["status"], row["reason"])
                } else if self.tab == 0 {
                    format!(
                        "{} | {} | {} | confidence {} | {} evidence",
                        row["memory"]["proposal"]["title"],
                        row["memory"]["status"],
                        row["memory"]["proposal"]["scope"],
                        row["memory"]["proposal"]["confidence"],
                        row["memory"]["proposal"]["evidence"]
                            .as_array()
                            .map(|a| a.len())
                            .unwrap_or(0)
                    )
                } else if self.tab == 1 {
                    format!(
                        "{} | {} | optional estimate {} tokens",
                        row["task_id"], row["phase"], row["optional_estimated_tokens"]
                    )
                } else if self.tab == 2 {
                    format!(
                        "{} | {} | {}",
                        row["tool_id"], row["state"], row["native_auto"]
                    )
                } else {
                    format!(
                        "{} | {} | {}",
                        row["provider"]["id"], row["status"], row["provider"]["version"]
                    )
                };
                lines.push(format!(
                    "{} {label}",
                    if i == self.selected { ">" } else { " " }
                ));
            }
        }
        if let Some(form) = &self.form {
            lines.push(format!(
                "{} ({}/{}): {}",
                form.name,
                form.values.len() + 1,
                form.labels.len(),
                form.labels[form.values.len()]
            ));
            lines.push(format!("> {}", form.input));
            lines.push("Enter next/prepare | Esc cancel. Reviewed writes require a separate a confirmation.".into());
        }
        lines.push(self.details.clone());
        f.render_widget(
            Paragraph::new(lines.join("\n"))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("O2 Memory / Context / Compaction / Code Intelligence"),
                )
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0)),
            area,
        );
    }
}

#[cfg(test)]
mod tests;

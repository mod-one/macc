//! Interactive session administration. Domain validation and mutations stay in core.
use crossterm::event::KeyCode;
use macc_core::session_store::{self as store, MigrationReport, SessionPolicy, StoreView};
use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use serde_json::Value;
use std::path::Path;
#[derive(Default)]
pub struct SessionsScreen {
    o2: crate::o2::Panel,
    usage: crate::session_usage::UsagePanel,
    checkpoints: crate::session_checkpoints::CheckpointPanel,
    pub view: Option<StoreView>,
    draft_revision: String,
    inherit: bool,
    last_refresh: Option<std::time::Instant>,
    scroll: u16,
    draft: Value,
    selected: usize,
    editing: Option<String>,
    pub message: String,
    plan: Option<MigrationReport>,
    closing: Option<String>,
    details: String,
}
impl SessionsScreen {
    pub fn refresh(&mut self, root: &Path) -> macc_core::Result<()> {
        let view = store::inspect(root)?;
        if self.view.is_none() {
            self.draft_revision = view.revision.clone();
            self.draft = serde_json::to_value(&view.policy).unwrap_or(Value::Null);
        }
        self.view = Some(view);
        self.last_refresh = Some(std::time::Instant::now());
        Ok(())
    }
    pub fn tick(&mut self, root: &Path) {
        self.o2.poll(root);
        if self
            .last_refresh
            .map(|t| t.elapsed().as_secs() >= 5)
            .unwrap_or(true)
        {
            if let Err(e) = self.refresh(root).and_then(|_| {
                if self.usage.open {
                    self.usage.refresh(root)
                } else {
                    Ok(())
                }
            }) {
                self.message = format!("Refresh failed; displayed data may be stale: {e}");
                self.last_refresh = Some(std::time::Instant::now());
            }
        }
    }
    fn path(&self) -> Option<String> {
        self.view
            .as_ref()?
            .fields
            .get(self.selected)
            .map(|f| format!("/{}", f.path.replace('.', "/")))
    }
    fn selected_id(&self) -> Option<String> {
        let v = self.view.as_ref()?;
        v.sessions
            .get(self.selected.checked_sub(v.fields.len())?)
            .map(|s| s.id.clone())
    }
    pub fn is_mutation(&self, key: KeyCode) -> bool {
        if self.o2.open {
            return self.o2.mutation(key);
        }
        self.usage.is_mutation(key)
            || self.checkpoints.is_mutation(key)
            || self.editing.is_none() && matches!(key, KeyCode::Char('s' | 'a' | 'v' | 'b'))
    }
    pub fn key(&mut self, root: &Path, key: KeyCode) -> macc_core::Result<bool> {
        if self.view.is_none() && !matches!(key, KeyCode::Esc | KeyCode::Char('q')) {
            self.refresh(root)?;
        }
        if self.o2.open {
            self.o2.key(root, key)?;
            return Ok(false);
        }
        if self.usage.open {
            self.usage.key(root, key)?;
            return Ok(false);
        }
        if self.checkpoints.open {
            if let Some(view) = self.view.as_ref() {
                self.checkpoints.key(root, key, view)?;
            }
            if key == KeyCode::Char('a') {
                self.refresh(root)?;
            }
            return Ok(false);
        }
        if let Some(input) = self.editing.as_mut() {
            match key {
                KeyCode::Esc => {
                    self.editing = None;
                }
                KeyCode::Enter => {
                    let value = input.parse::<u64>().map_err(|_| {
                        macc_core::MaccError::Validation("Enter a nonnegative whole number".into())
                    })?;
                    if let Some(path) = self.path() {
                        if let Some(target) = self.draft.pointer_mut(&path) {
                            *target = Value::from(value);
                        }
                    }
                    self.editing = None;
                    self.inherit = false;
                    self.plan = None;
                }
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(c) if c.is_ascii_digit() => input.push(c),
                _ => {}
            }
            return Ok(false);
        }
        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                let clean = !self.inherit
                    && self
                        .view
                        .as_ref()
                        .map(|v| serde_json::to_value(&v.policy).ok().as_ref() == Some(&self.draft))
                        .unwrap_or(true);
                if !clean {
                    self.message =
                        "Unsaved policy draft. Press x to discard or s to save before leaving."
                            .into();
                    return Ok(false);
                }
                return Ok(true);
            }
            KeyCode::Char('o') => {
                self.o2.refresh(root)?;
                self.o2.open = true;
            }
            KeyCode::Char('t') => {
                self.usage.refresh(root)?;
                self.usage.open = true;
            }
            KeyCode::Char('h') => self.checkpoints.open = true,
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => {
                let n = self
                    .view
                    .as_ref()
                    .map(|v| v.fields.len() + v.sessions.len())
                    .unwrap_or(0);
                self.selected = (self.selected + 1).min(n.saturating_sub(1));
            }
            KeyCode::Enter => {
                if let Some(path) = self.path() {
                    self.editing = Some(
                        self.draft
                            .pointer(&path)
                            .unwrap_or(&Value::Null)
                            .to_string(),
                    );
                } else if let Some(id) = self.selected_id() {
                    let selected = self
                        .view
                        .as_ref()
                        .and_then(|v| v.sessions.iter().find(|s| s.id == id));
                    let events = if self.view.as_ref().is_some_and(|v| v.backend == "sqlite") {
                        store::events(root, &id)?
                    } else {
                        Vec::new()
                    };
                    self.details = serde_json::to_string_pretty(
                        &serde_json::json!({"session":selected,"events":events}),
                    )
                    .unwrap_or_default();
                }
            }
            KeyCode::Char('r') => {
                self.refresh(root)?;
                self.message = "Refreshed; draft preserved. Save detects concurrent edits.".into();
            }
            KeyCode::Char('x') => {
                self.view = None;
                self.inherit = false;
                self.refresh(root)?;
                self.plan = None;
                self.closing = None;
                self.message = "Draft discarded.".into();
            }
            KeyCode::Char('u') => {
                self.draft = serde_json::to_value(SessionPolicy::default()).unwrap();
                self.inherit = true;
                self.message =
                    "Project override removal staged; p previews and s applies inherited defaults."
                        .into();
            }
            KeyCode::Char('d') => {
                self.inherit = false;
                self.draft = serde_json::to_value(SessionPolicy::default()).unwrap();
                self.message = "Defaults staged; press p to review, then s to apply.".into();
            }
            KeyCode::Char('p') => {
                let policy: SessionPolicy = serde_json::from_value(self.draft.clone())
                    .map_err(|e| macc_core::MaccError::Validation(e.to_string()))?;
                policy.validate()?;
                self.message = format!(
                    "Validated. Next session claim only. Before: {}\nAfter: {}",
                    serde_json::to_string(&self.view.as_ref().unwrap().policy).unwrap(),
                    self.draft
                );
            }
            KeyCode::Char('s') => {
                let policy: SessionPolicy = serde_json::from_value(self.draft.clone())
                    .map_err(|e| macc_core::MaccError::Validation(e.to_string()))?;
                let revision = self.draft_revision.clone();
                self.view = Some(if self.inherit {
                    store::remove_policy_override(root, &revision)?
                } else {
                    store::update_policy(root, &revision, &policy)?
                });
                self.inherit = false;
                self.draft_revision = self.view.as_ref().unwrap().revision.clone();
                self.message =
                    "Policy saved. Effective on the next session claim; active jobs unchanged."
                        .into();
            }
            KeyCode::Char('m') => {
                let sqlite = self.view.as_ref().is_some_and(|v| v.backend == "sqlite");
                self.plan = Some(if sqlite {
                    store::rollback(root, false, None)?
                } else {
                    store::migrate(root, false, None)?
                });
                self.closing = None;
                self.message="Review migration evidence below. Stop coordinator/supervisor/runners, then press a to apply.".into();
            }
            KeyCode::Char('c') => {
                self.closing = self.selected_id();
                self.plan = None;
                self.message = format!(
                    "Close session {:?}? Press a to confirm. Leased sessions cannot close.",
                    self.closing
                );
            }
            KeyCode::Char('a') => {
                if let Some(id) = self.closing.take() {
                    store::close(root, &id)?;
                    self.message = "Session closed.".into();
                } else if let Some(plan) = self.plan.as_ref() {
                    let result = if plan.direction == "json_to_sqlite" {
                        store::migrate(root, true, Some(&plan.source_sha256))?
                    } else {
                        store::rollback(root, true, Some(&plan.source_sha256))?
                    };
                    self.message =
                        format!("Authority changed. Evidence backup: {:?}", result.backup);
                    self.plan = None;
                }
                self.refresh(root)?;
            }
            KeyCode::Char('v') => {
                let n = store::recover(root)?;
                self.refresh(root)?;
                self.message = format!(
                    "Quarantined {n} expired dead-owner leases; no transcript was resumed."
                );
            }
            KeyCode::Char('b') => {
                self.message = format!("Consistent backup: {}", store::backup(root)?);
            }
            _ => {}
        }
        Ok(false)
    }
    pub fn draw(&self, f: &mut Frame, area: Rect) {
        if self.o2.open {
            self.o2.draw(f, area, &self.message);
            return;
        }
        if self.usage.open {
            self.usage.draw(f, area);
            return;
        }
        if self.checkpoints.open {
            self.checkpoints.draw(f, area, self.view.as_ref());
            return;
        }
        let mut lines=vec!["Sessions — o O2 | t usage | h checkpoints | r refresh | ↑↓ select | Enter edit/events | p validate/preview | s apply policy".into(),
            "u inherit defaults | PgUp/PgDn scroll | m preview migration/rollback | a apply reviewed action | c close | v recover | b backup | d defaults | x discard | Esc back".into()];
        if let Some(v) = &self.view {
            lines.push(format!(
                "Authority: {} | Project: {} | Policy changes activate at next claim",
                v.backend,
                v.project_id.as_deref().unwrap_or("legacy")
            ));
            lines.push(format!(
                "Policy source: {} | Staged override removal: {} | Draft revision current: {}",
                v.policy_source,
                self.inherit,
                v.revision == self.draft_revision
            ));
            for (i, field) in v.fields.iter().enumerate() {
                let path = format!("/{}", field.path.replace('.', "/"));
                let value = if i == self.selected {
                    self.editing.clone().unwrap_or_else(|| {
                        self.draft
                            .pointer(&path)
                            .unwrap_or(&Value::Null)
                            .to_string()
                    })
                } else {
                    self.draft
                        .pointer(&path)
                        .unwrap_or(&Value::Null)
                        .to_string()
                };
                lines.push(format!(
                    "{} {}: {} {} [{}..{}]",
                    if i == self.selected { ">" } else { " " },
                    field.label,
                    value,
                    field.unit,
                    field.min,
                    field.max
                ));
                if i == self.selected {
                    lines.push(format!("  {} ({})", field.help, field.path));
                }
            }
            lines.push(
                "Latest sessions: task | role | tool | status | generation | uses | lease owner / expiry"
                    .into(),
            );
            for (i, s) in v.sessions.iter().enumerate() {
                lines.push(format!(
                    "{} {} | {} | {} | {} | {} | {} | {:?}",
                    if i + v.fields.len() == self.selected {
                        ">"
                    } else {
                        " "
                    },
                    s.task_id,
                    s.role,
                    s.tool_id,
                    s.status,
                    s.generation,
                    s.use_count,
                    s.owner_pid
                ));
            }
        }
        if let Some(plan) = &self.plan {
            lines.push(serde_json::to_string_pretty(plan).unwrap());
        }
        lines.push(self.message.clone());
        lines.push(self.details.clone());
        f.render_widget(
            Paragraph::new(lines.join("\n"))
                .block(
                    Block::default()
                        .title("Session ownership")
                        .borders(Borders::ALL),
                )
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0)),
            area,
        );
    }
}

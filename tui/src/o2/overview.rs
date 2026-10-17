//! Task navigation and rendering; all counters and permissions come from core.
use super::*;
use macc_core::o2::overview::Query;
#[derive(Default)]
pub(super) struct Overview {
    pub query: Query,
    pub selected_task: Option<String>,
    pub snapshot: Option<Value>,
    pub last_refresh: Option<std::time::Instant>,
}
impl Panel {
    pub(super) fn refresh_overview(&mut self, root: &Path) -> Result<()> {
        if self.job.is_none() {
            self.overview.last_refresh = Some(std::time::Instant::now());
            self.launch(
                root,
                Action::Overview {
                    query: self.overview.query.clone(),
                },
            )?;
        }
        Ok(())
    }
    pub(super) fn overview_key(&mut self, root: &Path, key: KeyCode) -> Result<bool> {
        if self.tab != 5 {
            return Ok(false);
        }
        match key {
            KeyCode::Char('/') => self.start(
                "overview_filter",
                false,
                &["Exact task ID (blank for all tasks)"],
            ),
            KeyCode::Char(']') => {
                let next = self
                    .overview
                    .snapshot
                    .as_ref()
                    .and_then(|v| v["next_cursor"].as_str().map(str::to_string));
                if next.is_none() {
                    self.message = "No further task page.".into();
                    return Ok(true);
                }
                self.overview.query.after = next;
                self.refresh_overview(root)?;
            }
            KeyCode::Char('[') => {
                self.overview.query.after = None;
                self.refresh_overview(root)?;
            }
            KeyCode::Char('m' | 't' | 's') => {
                let task = self.selected()?["task_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                self.overview.selected_task = Some(task.clone());
                if key == KeyCode::Char('s') {
                    self.session_task = Some(task);
                    return Ok(true);
                }
                self.access = json!({"task_id":task});
                self.tab = if key == KeyCode::Char('m') { 0 } else { 1 };
                self.selected = 0;
                self.refresh(root)?;
                self.message =
                    format!("Task scope: {task}. 6 returns to overview; policy draft retained.");
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    pub(super) fn overview_filter(&mut self, root: &Path, task: &str) -> Result<()> {
        if task.len() > 256 {
            return Err(MaccError::Validation("Task ID exceeds 256 bytes".into()));
        }
        self.rows.clear();
        self.overview.snapshot = None;
        self.overview.query.task_id = (!task.trim().is_empty()).then(|| task.trim().to_string());
        self.overview.query.after = None;
        self.refresh_overview(root)
    }
    pub(super) fn overview_lines(&self, lines: &mut Vec<String>) {
        lines.push("/ exact task filter | [ first page | ] next page | Enter inspect | m scoped memory | t scoped context | s sessions/checkpoints | 5 policies".into());
        if let Some(v) = &self.overview.snapshot {
            lines.push(format!(
                "Snapshot {} | observed records; no delivery inference",
                v["captured_at"]
            ));
            lines.push(v["notice"].as_str().unwrap_or_default().into());
            lines.push(format!("Project memory statuses: {}", v["memory_statuses"]));
            lines.push(format!(
                "Recorded providers (not live health): {}",
                v["providers"]
            ));
            if self.rows.is_empty() {
                lines.push(
                    "No matching task evidence. Check task scope or wait for a checkpoint.".into(),
                );
            }
        }
        for (i, row) in self.rows.iter().enumerate() {
            lines.push(format!("{} {} | sessions {} / leases {} | checkpoints {} / reconcile {} | context {} | proposals {}",if i==self.selected{">"}else{" "},row["task_id"],row["sessions"],row["lease_records"],row["checkpoints"],row["needs_reconciliation"],row["contexts"],row["proposals"]));
        }
    }
    pub(super) fn replace_rows(&mut self, rows: Vec<Value>) {
        let key = |v: &Value| {
            if self.tab == 0 && self.conflicts {
                v["id"].clone()
            } else if self.tab == 0 {
                v["memory"]["id"].clone()
            } else if self.tab == 3 {
                v["provider"]["id"].clone()
            } else if self.tab == 5 {
                v["task_id"].clone()
            } else if self.tab == 2 {
                v["tool_id"].clone()
            } else {
                v["id"].clone()
            }
        };
        let selected = self.rows.get(self.selected).map(&key);
        let next = selected.and_then(|id| rows.iter().position(|r| key(r) == id));
        self.rows = rows;
        self.selected = next.unwrap_or(self.selected.min(self.rows.len().saturating_sub(1)));
    }
}

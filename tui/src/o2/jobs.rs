//! Keep provider processes off the terminal event loop after the caller's mutation gate.
use super::*;
pub(super) struct Job {
    receiver: std::sync::mpsc::Receiver<Result<Value>>,
    operation: Option<String>,
    providers: bool,
    memory: Option<Value>,
    details_scope: Option<Value>,
    applied_memory: bool,
    memory_action: Option<Value>,
    retry_form: Option<forms::Form>,
    overview: Option<macc_core::o2::overview::Query>,
    refreshed: std::time::Instant,
}
impl Panel {
    pub(super) fn launch(&mut self, root: &Path, action: Action) -> Result<()> {
        if self.job.is_some() {
            return Err(MaccError::Validation(
                "An O2 provider operation is already running; inspect its status first".into(),
            ));
        }
        let operation = if let Action::ProviderApply { plan_id, .. } = &action {
            Some(plan_id.clone())
        } else {
            None
        };
        let overview = if let Action::Overview { query } = &action {
            Some(query.clone())
        } else {
            None
        };
        let providers = matches!(&action, Action::Providers);
        let memory = matches!(&action, Action::MemorySearch { .. })
            .then(|| json!({"query":self.query,"access":self.access}));
        let details_scope = matches!(
            &action,
            Action::SemanticStatus { .. }
                | Action::SemanticBenchmark { .. }
                | Action::SemanticGate { .. }
                | Action::MemoryToolEvidence { .. }
                | Action::MemorySymbolEvidence { .. }
                | Action::MemoryPull { .. }
                | Action::MemoryReview { .. }
                | Action::MemoryResolveConflict { .. }
        )
        .then(|| self.access.clone());
        let applied_memory = matches!(
            &action,
            Action::SemanticBenchmark { .. }
                | Action::SemanticGate { .. }
                | Action::MemoryReview { .. }
                | Action::MemoryResolveConflict { .. }
                | Action::MemoryToolEvidence { .. }
        );
        let memory_action =
            applied_memory.then(|| serde_json::to_value(&action).unwrap_or_default());
        let root = root.to_path_buf();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(execute(&root, action));
        });
        self.job = Some(Job {
            receiver,
            operation,
            providers,
            memory,
            details_scope,
            applied_memory,
            memory_action,
            retry_form: None,
            overview,
            refreshed: std::time::Instant::now(),
        });
        self.message="Provider operation running. The terminal remains interactive; closing this panel does not cancel it.".into();
        Ok(())
    }
    pub(super) fn retain_job_form(&mut self, form: forms::Form) {
        if let Some(job) = self.job.as_mut() {
            job.retry_form = Some(form);
        }
    }
    pub fn poll(&mut self, root: &Path) {
        if self.open
            && self.tab == 5
            && self.job.is_none()
            && self
                .overview
                .last_refresh
                .is_none_or(|t| t.elapsed().as_secs() >= 5)
        {
            if let Err(e) = self.refresh_overview(root) {
                self.message = format!("Overview refresh failed; snapshot may be stale: {e}");
            }
        }
        let Some(job) = self.job.as_mut() else {
            return;
        };
        match job.receiver.try_recv() {
            Ok(result) => {
                let operation = job.operation.clone();
                let providers = job.providers;
                let overview = job.overview.clone();
                let memory = job.memory.clone();
                let details_scope = job.details_scope.clone();
                let applied_memory = job.applied_memory;
                let memory_action = job.memory_action.clone();
                let retry_form = job.retry_form.clone();
                self.job = None;
                if applied_memory
                    && result.is_ok()
                    && self
                        .pending
                        .as_ref()
                        .and_then(|a| serde_json::to_value(a).ok())
                        == memory_action
                {
                    self.pending = None;
                }
                let current = json!({"query":self.query,"access":self.access});
                if memory
                    .as_ref()
                    .is_some_and(|v| *v != current || self.tab != 0 || self.conflicts)
                    || details_scope
                        .as_ref()
                        .is_some_and(|v| *v != self.access || self.tab != 0)
                {
                    if self.tab == 0 {
                        let _ = self.refresh(root).map_err(|e| {
                            self.message = format!("Memory refresh failed: {e}");
                        });
                    }
                    return;
                }
                if result.is_err() && self.form.is_none() {
                    if let Some(mut f) = retry_form {
                        f.input = f.values.pop().unwrap_or_default();
                        self.form = Some(f);
                    }
                }
                match result {
                    Ok(value) if memory.is_some() => {
                        self.replace_rows(value.as_array().cloned().unwrap_or_default());
                        self.message = "Scoped memory refreshed.".into();
                    }
                    Ok(value) if overview.is_some() => {
                        if overview.as_ref() != Some(&self.overview.query) {
                            self.overview.last_refresh = None;
                        }
                        if self.tab == 5 && overview.as_ref() == Some(&self.overview.query) {
                            self.replace_rows(
                                value["tasks"].as_array().cloned().unwrap_or_default(),
                            );
                            self.overview.snapshot = Some(value);
                            self.message =
                                "Overview snapshot refreshed; no lifecycle actions executed."
                                    .into();
                        }
                    }
                    Ok(value) if providers => {
                        if self.tab == 3 {
                            self.replace_rows(
                                value["providers"].as_array().cloned().unwrap_or_default(),
                            );
                            self.selected = self.selected.min(self.rows.len().saturating_sub(1));
                            self.message = "Provider health refresh complete.".into();
                        }
                    }
                    Ok(value) => {
                        if self.pending.as_ref().is_some_and(|a| matches!(a,Action::ProviderApply{plan_id,..} if Some(plan_id)==operation.as_ref())) {self.pending=None;}
                        self.result(value);
                        self.message = "Provider operation complete.".into();
                        if operation.is_some() || applied_memory {
                            if let Err(e) = self.refresh(root) {
                                self.message = format!("Operation complete; refresh failed: {e}");
                            }
                        }
                    }
                    Err(e) if overview.is_some() => {
                        self.message =
                            format!("Overview refresh failed; previous snapshot is stale: {e}");
                    }
                    Err(e) => {
                        self.message =
                            format!("Provider operation failed; prior receipt retained: {e}")
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.job = None;
                self.message="Provider worker stopped unexpectedly; inspect operation progress and receipt before retrying.".into();
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                if job.refreshed.elapsed().as_secs() >= 2 {
                    job.refreshed = std::time::Instant::now();
                    if let Some(id) = &job.operation {
                        if let Ok(value) =
                            execute(root, Action::ProviderOperation { id: id.clone() })
                        {
                            self.result(value);
                        }
                    }
                }
            }
        }
    }
}

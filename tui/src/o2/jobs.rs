//! Keep provider processes off the terminal event loop after the caller's mutation gate.
use super::*;
pub(super) struct Job {
    receiver: std::sync::mpsc::Receiver<Result<Value>>,
    operation: Option<String>,
    providers: bool,
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
        let root = root.to_path_buf();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(execute(&root, action));
        });
        self.job = Some(Job {
            receiver,
            operation,
            providers,
            overview,
            refreshed: std::time::Instant::now(),
        });
        self.message="Provider operation running. The terminal remains interactive; closing this panel does not cancel it.".into();
        Ok(())
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
                self.job = None;
                match result {
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
                        if operation.is_some() {
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

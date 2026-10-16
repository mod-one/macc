//! Keep provider processes off the terminal event loop after the caller's mutation gate.
use super::*;
pub(super) struct Job {
    receiver: std::sync::mpsc::Receiver<Result<Value>>,
    operation: Option<String>,
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
        let root = root.to_path_buf();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(execute(&root, action));
        });
        self.job = Some(Job {
            receiver,
            operation,
            refreshed: std::time::Instant::now(),
        });
        self.message="Provider operation running. The terminal remains interactive; closing this panel does not cancel it.".into();
        Ok(())
    }
    pub fn poll(&mut self, root: &Path) {
        let Some(job) = self.job.as_mut() else {
            return;
        };
        match job.receiver.try_recv() {
            Ok(result) => {
                let operation = job.operation.clone();
                self.job = None;
                match result {
                    Ok(value) => {
                        if self.pending.as_ref().is_some_and(|a| matches!(a,Action::ProviderApply{plan_id,..} if Some(plan_id)==operation.as_ref())) {self.pending=None;}
                        self.result(value);
                        self.message = "Provider operation complete.".into();
                        if let Err(e) = self.refresh(root) {
                            self.message = format!("Operation complete; refresh failed: {e}");
                        }
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

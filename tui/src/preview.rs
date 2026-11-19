//! Cache materialization and planning may block on external sources. Deliver
//! only the result for the configuration revision the operator previewed.
use crate::state::AppState;
use macc_adapter_shared::fetch::materialize_fetch_units;
use macc_core::{
    config::CanonicalConfig,
    plan::PlannedOp,
    resolve::{resolve, resolve_fetch_units, CliOverrides},
};
use std::sync::mpsc::{sync_channel, Receiver, TryRecvError};

pub(crate) struct Job {
    config: CanonicalConfig,
    receiver: Receiver<Result<Vec<PlannedOp>, String>>,
}
impl AppState {
    pub fn refresh_preview_plan(&mut self) {
        if self.preview_job.is_some() {
            return;
        }
        self.preview_ops.clear();
        self.preview_diff_cache.clear();
        self.preview_diff_scroll.clear();
        self.preview_selection_index = 0;
        self.preview_error = None;
        if self.apply_preparing() {
            self.preview_error = Some(
                "Apply is still preparing shared sources. Refresh Preview when it finishes.".into(),
            );
            return;
        }
        let Some(paths) = self.project_paths.clone() else {
            self.preview_error = Some(
                "Preview requires a loaded MACC project. Run 'macc init' in the repo root.".into(),
            );
            return;
        };
        let Some(config) = self.working_copy.clone() else {
            self.preview_error = Some("No canonical configuration available to plan.".into());
            return;
        };
        let snapshot = config.clone();
        let engine = self.engine.clone();
        let (tx, receiver) = sync_channel(1);
        match std::thread::Builder::new()
            .name("macc-preview".into())
            .spawn(move || {
                let result = (|| {
                    let flags = CliOverrides::default();
                    let units = resolve_fetch_units(&paths, &resolve(&config, &flags))
                        .map_err(|e| format!("Failed to resolve catalog selections: {e}"))?;
                    let materials =
                        materialize_fetch_units(&paths, units, true, config.settings.offline)
                            .map_err(|e| format!("Failed to materialize catalog sources: {e}"))?;
                    let plan = engine
                        .plan(&paths, &config, &materials, &flags)
                        .map_err(|e| format!("Planning failed: {e}"))?;
                    Ok(engine.plan_operations(&paths, &plan))
                })();
                let _ = tx.send(result);
            }) {
            Ok(_) => {
                self.preview_job = Some(Job {
                    config: snapshot,
                    receiver,
                })
            }
            Err(e) => self.preview_error = Some(format!("Could not start preview: {e}")),
        }
    }
    pub(crate) fn poll_preview(&mut self) {
        let Some(job) = &self.preview_job else { return };
        let result=match job.receiver.try_recv() {
            Err(TryRecvError::Empty)=>return,
            Err(TryRecvError::Disconnected)=>Err("Preview worker stopped before delivering a plan.".into()),
            Ok(_) if self.working_copy.as_ref()!=Some(&job.config)=>Err("Configuration changed while preparing the preview. Refresh to review the current draft.".into()),
            Ok(result)=>result,
        };
        self.preview_job = None;
        match result {
            Ok(ops) => {
                self.preview_ops = ops;
                self.set_preview_selection(0);
            }
            Err(e) => self.preview_error = Some(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_preview_cannot_replace_a_changed_configuration() {
        let (_root, mut s) = crate::workspace_tests::fixture();
        let (tx, receiver) = sync_channel(1);
        s.preview_job = Some(Job {
            config: s.working_copy.clone().unwrap(),
            receiver,
        });
        s.working_copy.as_mut().unwrap().settings.quiet = true;
        tx.send(Ok(Vec::new())).unwrap();
        s.poll_preview();
        assert!(s.preview_job.is_none());
        assert!(s.preview_error.unwrap().contains("Configuration changed"));
        assert!(s.preview_ops.is_empty());
    }
}

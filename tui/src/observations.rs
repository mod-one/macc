//! Periodic read-only project observations. Rendering consumes the last completed snapshot.
use super::*;
use macc_core::{onboarding::ReadinessLadder, supervisor::live::SupervisorLive};
use std::{
    sync::mpsc::{sync_channel, Receiver, TryRecvError},
    time::Instant,
};

pub(crate) struct Snapshot {
    pub readiness: ReadinessLadder,
    pub trust: Option<String>,
    pub supervisor_status: Option<String>,
    pub intervention: Option<SupervisorLive>,
    pub prd: String,
}
#[derive(Default)]
pub(crate) struct Observations {
    pub latest: Option<Snapshot>,
    job: Option<Receiver<Snapshot>>,
    started: Option<Instant>,
}
impl AppState {
    pub(crate) fn poll_observations(&mut self) {
        let readiness_visible = self.current_screen() == Screen::Home && !self.home_activity;
        let obs = &mut self.observations;
        if let Some(rx) = &obs.job {
            match rx.try_recv() {
                Ok(snapshot) => {
                    if obs.latest.is_none() && readiness_visible {
                        self.inspector_index =
                            overview_view::next_step(&snapshot.readiness).unwrap_or(0);
                    }
                    obs.latest = Some(snapshot);
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {}
            }
            obs.job = None;
        }
        if obs
            .started
            .is_some_and(|t| t.elapsed() < Duration::from_secs(3))
        {
            return;
        }
        let Some(paths) = self.project_paths.clone() else {
            return;
        };
        let config = self.working_copy.clone();
        let engine = self.engine.clone();
        let running = self.is_coordinator_running();
        let (tx, rx) = sync_channel(1);
        self.observations.started = Some(Instant::now());
        match std::thread::Builder::new()
            .name("macc-tui-status".into())
            .spawn(move || {
                let readiness = engine.readiness_ladder(&paths);
                let trust = config.as_ref().and_then(|c| trust_warning_strip(&paths, c));
                let supervisor_status = supervisor::status_strip(&paths);
                let intervention = supervisor::live(&paths, running);
                let prd = macc_core::prd_queue::current(
                    &paths.root,
                    config
                        .as_ref()
                        .and_then(|c| c.automation.coordinator.as_ref()),
                )
                .label();
                let _ = tx.send(Snapshot {
                    readiness,
                    trust,
                    supervisor_status,
                    intervention,
                    prd,
                });
            }) {
            Ok(_) => self.observations.job = Some(rx),
            Err(e) => self.set_status(
                UiStatusLevel::Warning,
                format!("Project status refresh failed: {e}"),
                Some(Duration::from_secs(3)),
            ),
        }
    }
}

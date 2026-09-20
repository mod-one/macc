use super::errors::ApiError;
use super::WebState;
use macc_core::process_ownership::{ProcessHandle, ProcessKind};
use macc_core::service::coordinator::CoordinatorManagedCommandState;
use std::sync::atomic::Ordering;
use std::time::Duration;

const COORDINATOR_LEASE_POLL_INTERVAL: Duration = Duration::from_millis(250);

pub(super) fn spawn_release_monitor(state: WebState, client_id: String, generation: u64) {
    tokio::spawn(async move {
        loop {
            if state.coordinator_lease_generation.load(Ordering::SeqCst) != generation {
                return;
            }

            let paths = state.paths.clone();
            let engine = state.engine.clone();
            let coordinator_state = tokio::task::spawn_blocking(move || {
                engine.coordinator_managed_command_state(&paths)
            })
            .await;

            match coordinator_state {
                Ok(Ok(CoordinatorManagedCommandState::Running { .. })) => {}
                Ok(Ok(
                    CoordinatorManagedCommandState::Succeeded { .. }
                    | CoordinatorManagedCommandState::Failed { .. }
                    | CoordinatorManagedCommandState::Idle,
                )) => {
                    if let Err(err) = release_if_current(&state, &client_id, generation).await {
                        tracing::warn!(
                            client_id,
                            generation,
                            "failed to release web coordinator lease after exit: {:?}",
                            err
                        );
                    }
                    return;
                }
                Ok(Err(err)) => {
                    tracing::warn!(
                        client_id,
                        generation,
                        "failed to poll web-managed coordinator lease: {}",
                        err
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        client_id,
                        generation,
                        "web coordinator lease monitor join failed: {}",
                        err
                    );
                }
            }

            tokio::time::sleep(COORDINATOR_LEASE_POLL_INTERVAL).await;
        }
    });
}

pub(super) async fn release_if_current(
    state: &WebState,
    client_id: &str,
    generation: u64,
) -> Result<(), ApiError> {
    if state.coordinator_lease_generation.load(Ordering::SeqCst) != generation {
        return Ok(());
    }

    let paths = state.paths.clone();
    let engine = state.engine.clone();
    let client_id = client_id.to_string();
    tokio::task::spawn_blocking(move || {
        let handle = ProcessHandle {
            kind: ProcessKind::Coordinator,
            project_root: paths.root.clone(),
            pid: None,
        };
        engine.process_ownership_release(&paths.root, &handle, &client_id)
    })
    .await
    .map_err(|err| ApiError::validation(err.to_string()))??;
    Ok(())
}

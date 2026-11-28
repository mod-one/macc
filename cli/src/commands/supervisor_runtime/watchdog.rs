use chrono::{DateTime, Utc};
use macc_core::{
    coordinator::{managed_command_registry, state_runtime, tick_progress},
    coordinator_storage::{CoordinatorRun, CoordinatorStoragePaths, SqliteStorage},
    process_ownership::launched_group::Scope,
    ProjectPaths, Result,
};
use serde_json::json;
use std::time::Duration;

#[derive(PartialEq)]
pub(super) struct Observation {
    run_id: String,
    pid: i64,
    epoch: i64,
    tick: DateTime<Utc>,
    progress: Option<tick_progress::TickProgress>,
}

impl Observation {
    fn stale(paths: &ProjectPaths, run: &CoordinatorRun, window: u64) -> Result<Option<Self>> {
        let Some(tick) = run
            .last_tick_at
            .as_ref()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        else {
            return Ok(None);
        };
        let tick = tick.with_timezone(&Utc);
        let now = Utc::now();
        let overdue = |ms| {
            now.timestamp_millis().saturating_sub(ms) / 1000
                > i64::try_from(window.max(30)).unwrap_or(i64::MAX)
        };
        if !matches!(run.status.as_str(), "running" | "draining")
            || !overdue(tick.timestamp_millis())
        {
            return Ok(None);
        }
        let progress = tick_progress::read(&paths.root)?.filter(|p| {
            p.run_id == run.run_id && p.epoch == run.epoch && i64::from(p.pid) == run.pid
        });
        if progress.as_ref().is_some_and(|p| !overdue(p.since_ms))
            || state_runtime::read_coordinator_pause_file(&paths.root)?.is_some_and(|p| p.paused)
        {
            return Ok(None);
        }
        Ok(Some(Self {
            run_id: run.run_id.clone(),
            epoch: run.epoch,
            pid: run.pid,
            tick,
            progress,
        }))
    }
}

pub(super) fn inspect(
    paths: &ProjectPaths,
    run: Option<&CoordinatorRun>,
    window: u64,
    previous: &mut Option<Observation>,
) -> Result<bool> {
    let candidate = run
        .map(|r| Observation::stale(paths, r, window))
        .transpose()?
        .flatten();
    let confirmed = candidate.is_some() && candidate == *previous;
    *previous = candidate;
    if !confirmed {
        return Ok(false);
    }

    let store = SqliteStorage::read_only(CoordinatorStoragePaths::from_project_paths(paths));
    let registry = store.load_registry()?;
    let workers: Vec<_> = registry
        .tasks
        .iter()
        .filter(|task| task.task_runtime.pid.is_some())
        .map(|task| {
            json!({
                "task_id": task.id,
                "pid": task.task_runtime.pid,
                "last_heartbeat": task.task_runtime.last_heartbeat,
                "phase": task.current_phase()
            })
        })
        .collect();

    let Some(record) = managed_command_registry::get_managed_command(paths, "run")? else {
        return Ok(false);
    };
    let Some(group) = record
        .group
        .filter(|g| g.leader == record.pid && g.scope == Scope::CoordinatorSession)
    else {
        return Ok(false);
    };
    let _guard = tick_progress::lock(&paths.root)?;
    let Some(latest) = store.get_latest_coordinator_run()? else {
        return Ok(false);
    };
    let Some(observed) = Observation::stale(paths, &latest, window)? else {
        return Ok(false);
    };
    if previous.as_ref() != Some(&observed) || !group.is_running()? {
        return Ok(false);
    }
    // The managed command can be a wrapper. Its private session must contain the stalled engine.
    #[cfg(unix)]
    let owns_run = i32::try_from(latest.pid)
        .ok()
        .filter(|p| *p > 1)
        .is_some_and(|pid| unsafe { libc::getsid(pid) } == group.leader);
    #[cfg(not(unix))]
    let owns_run = false;
    if !owns_run {
        return Ok(false);
    }
    let now = Utc::now();
    let stage = observed
        .progress
        .as_ref()
        .map(|p| p.stage.as_str())
        .unwrap_or("unknown");
    let since = observed
        .progress
        .as_ref()
        .map_or(observed.tick.timestamp_millis(), |p| p.since_ms);
    let elapsed = now.timestamp_millis().saturating_sub(since) / 1000;
    if group.stop(false, Duration::from_millis(800))? > 0 {
        super::write_json(
            &paths.root.join(".macc/log/supervisor/stall.json"),
            &json!({"timestamp":now.to_rfc3339(),"run_id":latest.run_id,"pid":latest.pid,"epoch":latest.epoch,
                "last_tick_at":observed.tick.to_rfc3339(),"stage":stage,"stage_elapsed_seconds":elapsed,"workers":workers,
                "reason":format!("Supervisor stopped a coordinator whose tick exceeded the stall threshold ({stage}, {elapsed}s)") }),
        )?;
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn final_check_reads_the_live_tick_before_stopping_the_registered_session() {
        use macc_core::process_ownership::launched_group::LaunchedGroup;
        use managed_command_registry::{ManagedCommandRecord, ManagedCommandRegistry};
        use std::{os::unix::process::CommandExt, process::Command};

        let temp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(temp.path());
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        // SAFETY: setsid is async-signal-safe and isolates only this child.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        let mut child = cmd.spawn().unwrap();
        let pid = child.id() as i32;
        let mut record = ManagedCommandRecord::new(&paths, "run", pid);
        record.group = Some(LaunchedGroup::from_child(
            pid,
            "e67992b9-cde7-4d6b-8afa-0b87554ec932".parse().unwrap(),
            Scope::CoordinatorSession,
        ));
        ManagedCommandRegistry::new(&paths.root)
            .upsert(record)
            .unwrap();
        let old = (Utc::now() - chrono::Duration::minutes(20)).to_rfc3339();
        let stale = CoordinatorRun {
            run_id: "test".into(),
            epoch: 3,
            pid: i64::from(pid),
            status: "running".into(),
            hostname: "local".into(),
            started_at: old.clone(),
            last_tick_at: Some(old),
            stopped_at: None,
            stop_reason: None,
            version: "test".into(),
        };
        let store = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
        let mut latest = stale.clone();
        latest.last_tick_at = Some(Utc::now().to_rfc3339());
        store.upsert_coordinator_run(&latest).unwrap();
        let mut observation = None;
        assert!(!inspect(&paths, Some(&stale), 30, &mut observation).unwrap());
        assert!(!inspect(&paths, Some(&stale), 30, &mut observation).unwrap());
        assert!(child.try_wait().unwrap().is_none());
        assert!(!paths.root.join(".macc/log/supervisor/stall.json").exists());

        store.upsert_coordinator_run(&stale).unwrap();
        let mut observation = None;
        assert!(!inspect(&paths, Some(&stale), 30, &mut observation).unwrap());
        assert!(inspect(&paths, Some(&stale), 30, &mut observation).unwrap());
        assert!(!child.wait().unwrap().success());
        let bytes = std::fs::read(paths.root.join(".macc/log/supervisor/stall.json")).unwrap();
        let report: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(report["pid"], pid);
        assert_eq!(report["epoch"], 3);
        assert!(report["stage_elapsed_seconds"].as_i64().unwrap() > 30);
    }

    #[test]
    fn recent_stage_or_operator_pause_cancels_a_stale_tick() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(temp.path());
        let old = Utc::now() - chrono::Duration::minutes(20);
        let run = CoordinatorRun {
            run_id: "a".into(),
            epoch: 2,
            pid: 42,
            status: "running".into(),
            last_tick_at: Some(old.to_rfc3339()),
            hostname: "local".into(),
            started_at: old.to_rfc3339(),
            stopped_at: None,
            stop_reason: None,
            version: "test".into(),
        };
        assert!(Observation::stale(&paths, &run, 30).unwrap().is_some());
        let mut stage = tick_progress::TickProgress {
            run_id: run.run_id.clone(),
            epoch: 2,
            pid: 42,
            activity: None,
            process_identity: None,
            stage: "monitor performers".into(),
            since_ms: Utc::now().timestamp_millis(),
        };
        let path = paths.root.join(".macc/state/coordinator-tick.json");
        macc_core::atomic_write(&paths, &path, &serde_json::to_vec(&stage).unwrap()).unwrap();
        assert!(Observation::stale(&paths, &run, 30).unwrap().is_none());
        stage.epoch = 1;
        macc_core::atomic_write(&paths, &path, &serde_json::to_vec(&stage).unwrap()).unwrap();
        assert!(Observation::stale(&paths, &run, 30).unwrap().is_some());
        state_runtime::write_coordinator_pause_file(&paths.root, "T", "dev", "waiting").unwrap();
        assert!(Observation::stale(&paths, &run, 30).unwrap().is_none());
    }
}

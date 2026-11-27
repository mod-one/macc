//! Recover queue position from matching registry, archived series and decision plan.
//! Caller holds the queue lock until the cursor and registry agree.
use super::*;
use crate::config::{CoordinatorConfig, CoordinatorConfigResolved};
use crate::coordinator::{decisions, model::TaskRegistry};
use std::collections::BTreeSet;

pub(crate) fn align_locked(
    root: &Path,
    cfg: &CoordinatorConfig,
    selected: Option<&str>,
) -> Result<()> {
    let mut current = load(root)?;
    let registry = transition::registry(root, cfg)?;
    if current.entries.is_empty() || registry.tasks.is_empty() {
        return Ok(());
    }
    let open: Vec<_> = registry
        .tasks
        .iter()
        .filter(|t| !t.satisfies_dependants() && t.state != "abandoned")
        .collect();
    if open.is_empty() {
        return Ok(());
    }
    let entries = validate(root, &cfg.prd_files)?;
    if current.entries != entries {
        return Err(changed_error(&current.entries, &entries));
    }
    if let Some(entry) = current
        .entries
        .get(current.next)
        .filter(|e| open.iter().all(|t| e.task_ids.contains(&t.id)))
    {
        if selected.is_some_and(|path| path != entry.path) {
            return Err(invalid(format!(
                "Retained work belongs to {}. Use `macc coordinator resume`.",
                entry.path
            )));
        }
        return Ok(());
    }
    let history = root.join(".macc/state/prd-queue-history");
    let mut candidates = BTreeSet::new();
    if history.try_exists().map_err(|e| invalid(e.to_string()))? {
        for dir in std::fs::read_dir(&history).map_err(|e| invalid(e.to_string()))? {
            let dir = dir.map_err(|e| invalid(e.to_string()))?.path();
            if !dir.join("transition.json").is_file() {
                continue;
            } // incomplete archive
            let old: Progress = serde_json::from_slice(&read(&dir.join("progress.json"))?)
                .map_err(|e| invalid(e.to_string()))?;
            let Some(entry) = old.entries.get(old.next) else {
                continue;
            };
            if old.entries != entries
                || !registry
                    .tasks
                    .iter()
                    .all(|t| entry.task_ids.contains(&t.id))
            {
                continue;
            }
            let archived: TaskRegistry = serde_json::from_slice(&read(&dir.join("registry.json"))?)
                .map_err(|e| invalid(e.to_string()))?;
            if !open.iter().all(|task| {
                archived.find_task(&task.id).is_some_and(|old| {
                    old.task_runtime.run_id == task.task_runtime.run_id
                        && old.task_runtime.claim_id == task.task_runtime.claim_id
                        && old
                            .branch()
                            .zip(task.branch())
                            .is_none_or(|(old, new)| old == new)
                })
            }) {
                continue;
            }
            // Restoring the cursor also restores dependency credit for every prior PRD.
            let mut delivered = true;
            for (index, before) in entries.iter().take(old.next).enumerate() {
                let proof = dir.join(format!("prd-queue-completed-{index}.json"));
                if !proof.try_exists().map_err(|e| invalid(e.to_string()))? {
                    delivered = false;
                    break;
                }
                let evidence: TaskRegistry = serde_json::from_slice(&read(&proof)?)
                    .map_err(|e| invalid(format!("{}: {e}", proof.display())))?;
                if !incomplete_tasks(before, &evidence).is_empty() {
                    delivered = false;
                    break;
                }
            }
            if delivered {
                candidates.insert(old.next);
            }
        }
    }
    if cfg.decisions.as_ref().is_some_and(|p| p.enabled) {
        let store = decisions::Store::inspect_registered(root)?
            .ok_or_else(|| invalid("Missing durable decision plan for queue recovery"))?;
        let plans = store.plans()?;
        let reference = CoordinatorConfigResolved::resolve(Some(cfg)).reference_branch;
        let mut confirmed = BTreeSet::new();
        for i in candidates {
            let matches: Vec<_> = plans
                .iter()
                .filter(|p| {
                    registry
                        .extra
                        .get("decision_plan_id")
                        .and_then(|id| id.as_str())
                        .is_none_or(|id| id == p.id)
                        && p.source == entries[i].path
                        && p.reference_branch == reference
                        && open.iter().all(|t| {
                            t.task_runtime
                                .extra
                                .get("decision_context")
                                .and_then(|ctx| ctx["plan_id"].as_str())
                                .is_none_or(|id| id == p.id)
                        })
                })
                .collect();
            if let [plan] = matches.as_slice() {
                if store.projection_diff(root, plan)?["changed"] == false {
                    confirmed.insert(i);
                }
            }
        }
        candidates = confirmed;
    }
    let options = candidates
        .iter()
        .map(|i| entries[*i].path.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if let Some(path) = selected {
        candidates.retain(|i| entries[*i].path == path);
    }
    if candidates.len() != 1 {
        return Err(invalid(format!("{}: cannot identify one matching archived PRD. Verified candidates: [{}]. Select a listed PRD with `macc coordinator --prd PATH resume`; if none match, inspect queue history.",transition::CODE,options)));
    }
    let index = *candidates.first().unwrap();
    let message = format!(
        "Restored queue position from {} to {}/{} ({}) using retained tasks and archived progress",
        current.next + 1,
        index + 1,
        entries.len(),
        entries[index].path
    );
    current.next = index;
    current.status = "ready".into();
    current.reason = None;
    save(root, &current)?;
    crate::coordinator::helpers::append_coordinator_event_with_severity(
        root,
        "prd_queue_recovered",
        "-",
        "resume",
        "ready",
        &message,
        "info",
    )?;
    Ok(())
}

use crate::{fs_lock::AdvisoryLock, ProjectPaths};
use std::time::Duration;
const WAKE: &str = ".macc/state/resume-after-decision.json";

#[derive(Serialize, Deserialize, PartialEq)]
struct Wake {
    entry: Entry,
    reference: String,
    decisions: BTreeSet<String>,
}

fn running(paths: &ProjectPaths) -> Result<bool> {
    if crate::engine::check_coordinator_running_via_storage(paths) {
        return Ok(true);
    }
    Ok(
        crate::coordinator::managed_command_registry::get_managed_command(paths, "run")?
            .is_some_and(|r| crate::coordinator::helpers::is_pid_running(r.pid.into())),
    )
}

pub fn stopped(paths: &ProjectPaths, selected: Option<&str>) -> Result<Option<String>> {
    let outcome = resume_queue(paths, selected, None)?;
    // An answer may have committed just before the wait ticket was persisted.
    if outcome.is_some() && wake_after_decision(&paths.root)? {
        return Ok(Some("Decision recorded. Coordinator restarted.".into()));
    }
    Ok(outcome)
}

fn resume_queue(
    paths: &ProjectPaths,
    selected: Option<&str>,
    wake: Option<&Wake>,
) -> Result<Option<String>> {
    if !state_path(&paths.root).is_file() || running(paths)? {
        return Ok(None);
    }
    let config = crate::load_canonical_config(&paths.config_path)?;
    let Some(cfg) = config
        .automation
        .coordinator
        .as_ref()
        .filter(|c| !c.prd_files.is_empty())
    else {
        return Ok(None);
    };
    let launch = AdvisoryLock::acquire(
        &paths.root.join(".macc/state/supervisor-intervention.lock"),
        Duration::ZERO,
        "resume stopped queue",
    )?;
    if running(paths)? {
        return Ok(Some("Coordinator already running.".into()));
    }
    let queue = AdvisoryLock::acquire(
        &paths.root.join(".macc/state/prd-queue.lock"),
        Duration::ZERO,
        "recover stopped queue",
    )?;
    if let Some(expected) = wake {
        let progress = load(&paths.root)?;
        let path = paths.root.join(WAKE);
        if !path.try_exists().map_err(|e| invalid(e.to_string()))?
            || serde_json::from_slice::<Wake>(&read(&path)?).map_err(|e| invalid(e.to_string()))?
                != *expected
            || progress.entries.get(progress.next) != Some(&expected.entry)
        {
            return Ok(None);
        }
    }
    let previous = load(&paths.root)?.next;
    align_locked(&paths.root, cfg, selected)?;
    let progress = load(&paths.root)?;
    let Some(entry) = progress.entries.get(progress.next) else {
        return Ok(Some("No remaining PRD in this queue.".into()));
    };
    let position = format!(
        "{}PRD {}/{} ({})",
        if progress.next != previous {
            "Restored "
        } else {
            ""
        },
        progress.next + 1,
        progress.entries.len(),
        entry.path
    );
    let mut registry = transition::registry(&paths.root, cfg)?;
    let reference = CoordinatorConfigResolved::resolve(Some(cfg)).reference_branch;
    let mut args = Default::default();
    crate::service::coordinator_workflow::apply_storage_mode_args(
        &mut args,
        &Default::default(),
        Some(cfg),
    );
    let waits = super::manual_delivery::reconcile(paths, &mut registry, cfg, &args)?;
    let automatic = config
        .automation
        .supervisor
        .unwrap_or_default()
        .auto_restart;
    let wake_path = paths.root.join(WAKE);
    let messages = waits
        .iter()
        .map(|(_, msg)| msg.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if !automatic {
        cancel_wait(&paths.root)?;
    }
    if !waits.is_empty() && automatic {
        let ticket = Wake {
            entry: entry.clone(),
            reference,
            decisions: waits.iter().map(|(id, _)| id.clone()).collect(),
        };
        crate::atomic_write(
            paths,
            &wake_path,
            &serde_json::to_vec(&ticket).map_err(|e| invalid(e.to_string()))?,
        )?;
    }
    if !waits.is_empty()
        && wake.is_none()
        && registry.tasks.iter().all(|t| {
            t.satisfies_dependants()
                || matches!(
                    t.state.as_str(),
                    "abandoned" | "blocked" | "pending_approval"
                )
        })
    {
        let next = if automatic {
            "MACC will resume when the decision allows further work."
        } else {
            "After resolving the decision, run `macc coordinator resume`."
        };
        return Ok(Some(format!("{position}\n{messages}\n{next}")));
    }
    crate::coordinator::state_runtime::clear_coordinator_pause_file(&paths.root)?;
    drop(queue);
    crate::service::coordinator::coordinator_restart_after_intervention(paths, Some(cfg), &launch)?;
    if waits.is_empty() && automatic {
        cancel_wait(&paths.root)?;
    }
    Ok(Some(
        format!("Coordinator started on {position}. {messages}")
            .trim_end()
            .into(),
    ))
}

pub(crate) fn cancel_wait(root: &Path) -> Result<()> {
    let file = root.join(WAKE);
    match std::fs::remove_file(&file) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(invalid(format!("{}: {e}", file.display()))),
    }
}

/// A committed answer wakes only an explicitly resumed series. The supervisor
/// also retries this check after a crash between the answer and process launch.
pub fn wake_after_decision(root: &Path) -> Result<bool> {
    let path = root.join(WAKE);
    if !path.try_exists().map_err(|e| invalid(e.to_string()))? {
        return Ok(false);
    }
    let ticket: Wake = serde_json::from_slice(&read(&path)?).map_err(|e| invalid(e.to_string()))?;
    let cfg = crate::load_canonical_config(&ProjectPaths::from_root(root).config_path)?;
    if !cfg.automation.supervisor.unwrap_or_default().auto_restart {
        return Ok(false);
    }
    let progress = load(root)?;
    if !progress
        .entries
        .get(progress.next)
        .is_some_and(|e| e == &ticket.entry)
    {
        return Ok(false);
    }
    if running(&ProjectPaths::from_root(root))? {
        return Ok(false);
    }
    let Some(store) = decisions::Store::inspect_registered(root)? else {
        return Ok(false);
    };
    let mut actionable = false;
    for id in &ticket.decisions {
        let d = store.decision(id)?;
        let plan = store.plan(&d.plan_id)?;
        if plan.source != ticket.entry.path || plan.reference_branch != ticket.reference {
            return Ok(false);
        }
        actionable |= matches!(
            d.status.as_str(),
            "resolved" | "superseded" | "awaiting_effects" | "investigating_requested"
        );
    }
    if !actionable {
        return Ok(false);
    }
    let paths = ProjectPaths::from_root(root);
    resume_queue(&paths, None, Some(&ticket))?;
    running(&paths)
}

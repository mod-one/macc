//! Attribute recorded decision waits for diagnostics. Never change scheduling
//! state or treat a propagated wait as evidence of completion.
use super::model::{Task, TaskRegistry};
use super::phase_diagnostics::decision_wait_id as wait_id;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

pub(crate) struct DecisionWaits<'a> {
    consumers: HashSet<&'a str>,
    // Keep each distinct request once, even if many consumers report it.
    owners: HashMap<&'a str, BTreeMap<&'a str, &'a Task>>,
}

fn reason(witness: &Task) -> Option<&str> {
    witness
        .task_runtime
        .extra
        .get("decision_wait_reason")
        .and_then(|v| v.as_str())
        .or(witness.task_runtime.last_error_message.as_deref())
}

impl<'a> DecisionWaits<'a> {
    pub fn new(registry: &'a TaskRegistry) -> Self {
        let tasks: HashMap<_, _> = registry.tasks.iter().map(|t| (t.id.as_str(), t)).collect();
        let mut waits = Self {
            consumers: HashSet::new(),
            owners: HashMap::new(),
        };
        for task in &registry.tasks {
            let bound_performer = matches!(task.state.as_str(), "blocked" | "todo")
                && task.task_runtime.last_error_origin.as_deref() == Some("precondition")
                && task
                    .task_runtime
                    .extra
                    .get("decision_wait_owner")
                    .and_then(|v| v.as_str())
                    == Some(task.id.as_str())
                && task.task_runtime.extra.contains_key("decision_wait_reason");
            if task.task_runtime.last_error_code.as_deref() != Some("E903")
                || !(bound_performer
                    || task.state == "blocked"
                        && task.task_runtime.last_error_origin.as_deref() == Some("decisions"))
            {
                continue;
            }
            let Some(id) = wait_id(task) else { continue };
            let owner = task
                .task_runtime
                .extra
                .get("decision_wait_owner")
                .and_then(|v| v.as_str());
            let root = match owner.filter(|s| !s.is_empty()) {
                Some(owner) => {
                    let Some(root) = tasks.get(owner) else {
                        continue;
                    };
                    // A worker can finish after the native wait was projected:
                    // the owner then carries a performer precondition, possibly
                    // requeued to todo, while consumers retain the decision ID.
                    if !matches!(root.state.as_str(), "blocked" | "todo")
                        || root.task_runtime.last_error_code.as_deref() != Some("E903")
                        || !matches!(
                            root.task_runtime.last_error_origin.as_deref(),
                            Some("decisions" | "precondition")
                        )
                        || root
                            .task_runtime
                            .extra
                            .get("decision_wait")
                            .is_some_and(|v| v.as_str() != Some(id))
                    {
                        continue;
                    }
                    *root
                }
                None => legacy_wait_root(task, id, &tasks, &mut HashSet::new()),
            };
            if root.id != task.id {
                waits.consumers.insert(task.id.as_str());
            }
            waits
                .owners
                .entry(&root.id)
                .or_default()
                .entry(id)
                .or_insert(task);
        }
        waits
    }

    pub fn is_consumer(&self, task: &Task) -> bool {
        self.consumers.contains(task.id.as_str())
    }

    pub fn is_owner(&self, task: &Task) -> bool {
        self.owners.contains_key(task.id.as_str())
    }

    fn requests(&self, task: &Task) -> impl Iterator<Item = (&str, &Task)> {
        self.owners
            .get(task.id.as_str())
            .into_iter()
            .flatten()
            .map(|(&id, &witness)| (id, witness))
    }

    /// Build a diagnostic copy only when a native wait adds information. Real
    /// task state, assignment and performer-supplied evidence stay intact.
    pub fn reported_task(&self, task: &'a Task) -> Cow<'a, Task> {
        if !self.is_owner(task) {
            return Cow::Borrowed(task);
        }
        let mut reported = task.clone();
        let runtime = &mut reported.task_runtime;
        let needs_conditions = runtime.unmet_preconditions.is_empty();
        for (id, witness) in self.requests(task) {
            // Bind only the diagnostic snapshot when an older owner row has
            // no binding after completion. Never persist inferred metadata.
            if !runtime.extra.contains_key("decision_wait") {
                runtime.extra.insert("decision_wait".into(), id.into());
                runtime
                    .extra
                    .insert("decision_wait_owner".into(), task.id.clone().into());
            }
            let Some(reason) = reason(witness) else {
                continue;
            };
            if needs_conditions && !runtime.unmet_preconditions.iter().any(|r| r == reason) {
                runtime.unmet_preconditions.push(reason.into());
            }
            if super::result_protocol::missing_precondition_explanation(
                runtime.result_explanation.as_deref(),
            ) && needs_conditions
            {
                runtime.result_explanation = Some(reason.into());
                runtime.last_error_message = Some(reason.into());
            }
        }
        Cow::Owned(reported)
    }

    pub fn next_action(&self, task: &Task) -> Option<String> {
        self.is_owner(task).then(|| {
            self.requests(task)
                .map(|(id, witness)| {
                    let action = witness
                        .task_runtime
                        .extra
                        .get("decision_wait_action")
                        .and_then(|v| v.as_str())
                        .or(reason(witness))
                        .unwrap_or("Review the pending request");
                    format!("Open Decisions for {} ({}). {}", task.id, id, action)
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
    }
}

// Older projections without an owner can only be grouped through a
// dependency which records the very same decision.
fn legacy_wait_root<'a>(
    task: &'a Task,
    id: &str,
    tasks: &HashMap<&str, &'a Task>,
    seen: &mut HashSet<&'a str>,
) -> &'a Task {
    seen.insert(&task.id);
    for dependency in task.dependency_ids() {
        if let Some(&parent) = tasks.get(dependency.as_str()) {
            if !seen.contains(parent.id.as_str())
                && parent.state == "blocked"
                && parent.task_runtime.last_error_code.as_deref() == Some("E903")
                && parent.task_runtime.last_error_origin.as_deref() == Some("decisions")
                && wait_id(parent) == Some(id)
            {
                return legacy_wait_root(parent, id, tasks, seen);
            }
        }
    }
    task
}

#[cfg(test)]
mod tests;

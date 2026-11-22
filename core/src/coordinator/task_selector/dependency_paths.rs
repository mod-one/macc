//! Paths through unsatisfied dependencies, in declared order.
use super::Task;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

pub(super) fn is_blocked_dependency(task: &Task) -> bool {
    matches!(
        task.workflow_state(),
        Some(
            crate::coordinator::WorkflowState::Blocked
                | crate::coordinator::WorkflowState::Rejected
        )
    ) || (task.is_merged() && !task.gate_verdict_satisfies_dependencies())
}

/// Yield one path per matching prerequisite. Reconciliation needs the first
/// blocked root, while a diagnosis can list every outstanding approval.
pub(super) fn dependency_paths<'a>(
    task: &'a Task,
    tasks_by_id: &'a HashMap<&'a str, &'a Task>,
    satisfied_ids: &'a HashSet<String>,
    matches_root: fn(&Task) -> bool,
) -> impl Iterator<Item = Vec<&'a str>> + 'a {
    let mut pending = vec![(task, None)];
    let mut parents = HashMap::new();
    std::iter::from_fn(move || {
        while let Some((current, parent)) = pending.pop() {
            let id = current.id.as_str();
            let Entry::Vacant(entry) = parents.entry(id) else {
                continue;
            };
            entry.insert(parent);
            if parent.is_some() && matches_root(current) {
                let mut path: Vec<_> =
                    std::iter::successors(Some(id), |node| parents[node]).collect();
                path.reverse();
                return Some(path);
            }
            for dependency in current.dependencies.iter().rev() {
                if !satisfied_ids.contains(dependency) {
                    if let Some(&next) = tasks_by_id.get(dependency.as_str()) {
                        pending.push((next, Some(id)));
                    }
                }
            }
        }
        None
    })
}

use super::WebState;
use axum::extract::{Query, State};
use axum::Json;
use macc_core::runtime::RuntimeSnapshotBuilder;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub(crate) struct SearchQuery {
    q: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct SearchResult {
    kind: String,
    id: String,
    label: String,
    meta: Option<String>,
}

pub(crate) async fn search_handler(
    State(state): State<WebState>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<Vec<SearchResult>>, super::errors::ApiError> {
    let q = params.q.unwrap_or_default().to_lowercase();
    if q.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let mut results: Vec<SearchResult> = Vec::new();

    // Search tasks from snapshot
    if let Ok(snapshot) = RuntimeSnapshotBuilder::build(&state.paths) {
        for task in &snapshot.tasks {
            let id_match = task.task_id.to_lowercase().contains(&q);
            let title_match = task.title.to_lowercase().contains(&q);
            if id_match || title_match {
                results.push(SearchResult {
                    kind: "task".to_string(),
                    id: task.task_id.clone(),
                    label: format!("{} — {}", task.task_id, task.title),
                    meta: Some(format!(
                        "{} · {}",
                        task.workflow_state, task.runtime_status
                    )),
                });
            }
        }

        // Search worktrees
        for worker in &snapshot.workers {
            if worker.id.to_lowercase().contains(&q)
                || worker.task_id.as_deref().map(|t| t.to_lowercase().contains(&q)).unwrap_or(false)
            {
                results.push(SearchResult {
                    kind: "worktree".to_string(),
                    id: worker.id.clone(),
                    label: format!("Worker {} ({})", worker.id, worker.tool),
                    meta: worker.task_id.clone(),
                });
            }
        }

        // Search throttled tools / error codes
        for tool in &snapshot.throttled_tools {
            if tool.tool.to_lowercase().contains(&q)
                || tool.error_code.to_lowercase().contains(&q)
            {
                results.push(SearchResult {
                    kind: "error_code".to_string(),
                    id: tool.error_code.clone(),
                    label: format!("{} — {}", tool.error_code, tool.tool),
                    meta: Some(format!("throttled, backoff {}s", tool.backoff_seconds)),
                });
            }
        }
    }

    // Search skills
    let skills = macc_core::skills_runner::SkillResolver::list(&state.paths.macc_dir);
    for skill in &skills {
        if skill.id.to_lowercase().contains(&q)
            || skill.title.to_lowercase().contains(&q)
            || skill.description.to_lowercase().contains(&q)
        {
            results.push(SearchResult {
                kind: "skill".to_string(),
                id: skill.id.clone(),
                label: format!("{} — {}", skill.id, skill.title),
                meta: Some(skill.kind.as_str().to_string()),
            });
        }
    }

    results.truncate(20);
    Ok(Json(results))
}

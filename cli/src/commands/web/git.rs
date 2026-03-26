use super::errors::ApiError;
use super::WebState;
use axum::extract::{Query, State};
use axum::Json;
use macc_core::commit_message;
use macc_core::git;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const DEFAULT_GRAPH_LIMIT: usize = 100;
const MAX_GRAPH_LIMIT: usize = 500;
const SHORT_SHA_LEN: usize = 8;

#[derive(Debug, Deserialize)]
pub(super) struct GitGraphQuery {
    limit: Option<usize>,
    since: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct ApiGitGraphResponse {
    pub commits: Vec<ApiGitCommit>,
    pub branches: Vec<String>,
    pub head: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ApiGitCommit {
    pub sha: String,
    pub short_sha: String,
    pub subject: String,
    pub author: String,
    pub timestamp: i64,
    pub parent_shas: Vec<String>,
    pub branch_refs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

pub(super) async fn get_git_graph_handler(
    State(state): State<WebState>,
    Query(query): Query<GitGraphQuery>,
) -> std::result::Result<Json<ApiGitGraphResponse>, ApiError> {
    let limit = normalize_limit(query.limit)?;
    let since = query
        .since
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let commits = git::git_log_graph(&state.paths.root, limit, since).map_err(ApiError::from)?;
    let head = git::current_branch_name(&state.paths.root).unwrap_or_default();

    let mut branches = BTreeSet::new();
    let commits = commits
        .into_iter()
        .map(|commit| {
            for branch in &commit.refs {
                branches.insert(branch.clone());
            }
            let full_message = if commit.body.is_empty() {
                commit.subject.clone()
            } else {
                format!("{}\n\n{}", commit.subject, commit.body)
            };
            let parsed = commit_message::parse(&full_message);
            ApiGitCommit {
                short_sha: commit.sha.chars().take(SHORT_SHA_LEN).collect(),
                sha: commit.sha,
                subject: commit.subject,
                author: commit.author,
                timestamp: commit.timestamp,
                parent_shas: commit.parents,
                branch_refs: commit.refs,
                task_id: parsed.task_id,
            }
        })
        .collect();

    Ok(Json(ApiGitGraphResponse {
        commits,
        branches: branches.into_iter().collect(),
        head,
    }))
}

fn normalize_limit(limit: Option<usize>) -> Result<usize, ApiError> {
    match limit.unwrap_or(DEFAULT_GRAPH_LIMIT) {
        0 => Err(ApiError::from(macc_core::MaccError::Validation(
            "query parameter 'limit' must be greater than 0".to_string(),
        ))),
        value if value > MAX_GRAPH_LIMIT => {
            Err(ApiError::from(macc_core::MaccError::Validation(format!(
                "query parameter 'limit' must be less than or equal to {}",
                MAX_GRAPH_LIMIT
            ))))
        }
        value => Ok(value),
    }
}

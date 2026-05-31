use super::WebState;
use axum::extract::{Path, State};
use axum::Json;
use macc_core::engine::Engine;
use macc_core::skills_runner::SkillRunRequest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Serialize)]
pub(crate) struct SkillListItem {
    id: String,
    title: String,
    kind: String,
    risk: String,
    description: String,
}

#[derive(Deserialize)]
pub(crate) struct RunSkillRequest {
    tool: Option<String>,
    task_id: Option<String>,
    yes: Option<bool>,
}

pub(crate) async fn list_skills_handler(
    State(state): State<WebState>,
) -> Json<Vec<SkillListItem>> {
    let skills = state.engine.list_skills(&state.paths);
    Json(
        skills
            .into_iter()
            .map(|s| SkillListItem {
                id: s.id,
                title: s.title,
                kind: s.kind.as_str().to_string(),
                risk: s.risk.as_str().to_string(),
                description: s.description,
            })
            .collect(),
    )
}

pub(crate) async fn get_skill_handler(
    State(state): State<WebState>,
    Path(skill_id): Path<String>,
) -> Result<Json<Value>, super::errors::ApiError> {
    match state.engine.resolve_skill(&state.paths, &skill_id) {
        Some(skill) => {
            let v = serde_json::to_value(&skill)
                .map_err(|e| super::errors::ApiError::validation(e.to_string()))?;
            Ok(Json(v))
        }
        None => Err(super::errors::ApiError::not_found(
            format!("Skill '{}' not found", skill_id),
            None,
        )),
    }
}

pub(crate) async fn dry_run_skill_handler(
    State(state): State<WebState>,
    Path(skill_id): Path<String>,
) -> Result<Json<Value>, super::errors::ApiError> {
    let skill = state
        .engine
        .resolve_skill(&state.paths, &skill_id)
        .ok_or_else(|| {
            super::errors::ApiError::not_found(
                format!("Skill '{}' not found", skill_id),
                None,
            )
        })?;

    let preview = state.engine.dry_run_skill(&state.paths, &skill, None);

    let v = serde_json::to_value(&preview)
        .map_err(|e| super::errors::ApiError::validation(e.to_string()))?;
    Ok(Json(v))
}

pub(crate) async fn run_skill_handler(
    State(state): State<WebState>,
    Path(skill_id): Path<String>,
    Json(body): Json<RunSkillRequest>,
) -> Result<Json<Value>, super::errors::ApiError> {
    let skill = state
        .engine
        .resolve_skill(&state.paths, &skill_id)
        .ok_or_else(|| {
            super::errors::ApiError::not_found(
                format!("Skill '{}' not found", skill_id),
                None,
            )
        })?;

    let request = SkillRunRequest {
        skill_id: skill.id.clone(),
        tool_id: body.tool,
        cwd: state.paths.root.clone(),
        task_id: body.task_id,
        scope: None,
        inputs: HashMap::new(),
        dry_run: false,
        watch: false,
        yes: body.yes.unwrap_or(true),
    };

    let result = state
        .engine
        .run_skill(&state.paths, &skill, &request)
        .map_err(|e| super::errors::ApiError::validation(e.to_string()))?;

    let v = serde_json::to_value(&result)
        .map_err(|e| super::errors::ApiError::validation(e.to_string()))?;
    Ok(Json(v))
}

// ── Skill run log endpoints ────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct RunEntry {
    id: String,
    skill_id: String,
    started_at: String,
    status: String,
}

pub(crate) async fn list_runs_handler(
    State(state): State<WebState>,
) -> Json<Vec<RunEntry>> {
    let run_dir = state.paths.macc_dir.join("log").join("run");
    if !run_dir.exists() {
        return Json(Vec::new());
    }

    let mut entries: Vec<RunEntry> = std::fs::read_dir(&run_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .map(|x| x == "jsonl")
                .unwrap_or(false)
        })
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let stem = name.trim_end_matches(".jsonl");
            let parts: Vec<&str> = stem.splitn(2, '-').collect();
            if parts.len() < 2 {
                return None;
            }
            let content = std::fs::read_to_string(e.path()).ok()?;
            let last_line = content.lines().last()?;
            let v: serde_json::Value = serde_json::from_str(last_line).ok()?;
            let status = v
                .get("status")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown")
                .to_string();
            Some(RunEntry {
                id: stem.to_string(),
                skill_id: parts[1].to_string(),
                started_at: parts[0].to_string(),
                status,
            })
        })
        .collect();

    entries.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    entries.truncate(50);
    Json(entries)
}

pub(crate) async fn get_run_handler(
    State(state): State<WebState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, super::errors::ApiError> {
    let run_dir = state.paths.macc_dir.join("log").join("run");
    let jsonl = run_dir.join(format!("{}.jsonl", run_id));
    if !jsonl.exists() {
        return Err(super::errors::ApiError::not_found(
            format!("Run '{}' not found", run_id),
            None::<serde_json::Value>,
        ));
    }
    let content = std::fs::read_to_string(&jsonl)
        .map_err(|e| super::errors::ApiError::validation(e.to_string()))?;
    let lines: Vec<Value> = content
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    Ok(Json(serde_json::json!({ "run_id": run_id, "events": lines })))
}

pub(crate) async fn get_run_logs_handler(
    State(state): State<WebState>,
    Path(run_id): Path<String>,
) -> Result<String, super::errors::ApiError> {
    let run_dir = state.paths.macc_dir.join("log").join("run");
    let jsonl = run_dir.join(format!("{}.jsonl", run_id));
    if !jsonl.exists() {
        return Err(super::errors::ApiError::not_found(
            format!("Run '{}' not found", run_id),
            None::<serde_json::Value>,
        ));
    }
    std::fs::read_to_string(&jsonl)
        .map_err(|e| super::errors::ApiError::validation(e.to_string()))
}

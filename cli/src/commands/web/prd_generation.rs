use crate::commands::web::{errors::ApiError, WebState};
use axum::extract::{Path, State};
use axum::Json;
use macc_core::prd_generation::{
    audit::PrdAuditRequest,
    metadata::PRD_GENERATION_DEFAULT_TARGET_DIR,
    promotion::PromoteOptions,
    request::{ModelRoutingMode, ModelSelection},
    PromoteResult, ValidationResult,
};
use serde::{Deserialize, Serialize};

// ── Request / response DTOs ───────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPrdGenerateRequest {
    pub from_path: String,
    pub tool: Option<String>,
    pub model_selection: Option<ApiModelSelection>,
    pub instructions: Option<String>,
    pub instructions_file: Option<String>,
    pub target_dir: Option<String>,
    pub update_path: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub promote: bool,
    #[serde(default)]
    pub yes: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPrdAuditRequest {
    pub prd_path: Option<String>,
    pub tool: Option<String>,
    pub model_selection: Option<ApiModelSelection>,
    pub instructions: Option<String>,
    pub instructions_file: Option<String>,
    pub reference_branch: Option<String>,
    #[serde(default)]
    pub diff_stat: bool,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPrdPromoteRequest {
    pub source_path: String,
    pub dest_path: Option<String>,
    #[serde(default)]
    pub yes: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPrdValidateRequest {
    pub prd_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiModelSelection {
    pub mode: String,
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPrdGenerateResponse {
    pub status: String,
    pub run_id: Option<String>,
    pub target_dir: Option<String>,
    pub tool: Option<String>,
    pub prompt: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPrdAuditResponse {
    pub completed_with_context: usize,
    pub todo_tasks: usize,
    pub prompt_generated: bool,
    pub prompt: Option<String>,
    pub dispatched: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPrdRunEntry {
    pub run_id: String,
    pub path: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

pub(crate) async fn prd_generate_handler(
    State(state): State<WebState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ApiPrdGenerateRequest>,
) -> std::result::Result<Json<ApiPrdGenerateResponse>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let paths = state.paths.clone();
    let engine = state.engine.clone();

    tokio::task::spawn_blocking(move || {
        let from = paths.root.join(&req.from_path);
        if !from.exists() {
            return Err(ApiError::validation(format!(
                "PRD-GEN-INPUT-MISSING: brief file '{}' not found",
                from.display()
            )));
        }

        let brief_content = std::fs::read_to_string(&from)
            .map_err(|e| ApiError::validation(e.to_string()))?;

        let model_sel = req.model_selection.as_ref().map(to_model_selection);

        let run_meta = macc_core::prd_generation::metadata::GenerationRunMetadata::new(
            &req.from_path,
            req.tool.clone(),
            req.dry_run,
        );
        let run_dir = macc_core::prd_generation::target_dir::resolve_target_dir(
            &paths.root,
            req.target_dir.as_deref().map(std::path::Path::new),
            &run_meta.run_id,
        )
        .map_err(|e| ApiError::validation(e))?;

        let prompt = engine.prd_build_generate_prompt(
            &brief_content,
            &run_dir,
            None,
            req.instructions.as_deref(),
            model_sel.as_ref(),
        );

        if req.dry_run {
            return Ok(Json(ApiPrdGenerateResponse {
                status: "dry_run".into(),
                run_id: Some(run_meta.run_id),
                target_dir: Some(run_dir.to_string_lossy().to_string()),
                tool: req.tool,
                prompt: Some(prompt),
            }));
        }

        Ok(Json(ApiPrdGenerateResponse {
            status: "prompt_ready".into(),
            run_id: Some(run_meta.run_id),
            target_dir: Some(run_dir.to_string_lossy().to_string()),
            tool: req.tool,
            prompt: Some(prompt),
        }))
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))?
}

pub(crate) async fn prd_audit_handler(
    State(state): State<WebState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ApiPrdAuditRequest>,
) -> std::result::Result<Json<ApiPrdAuditResponse>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let paths = state.paths.clone();
    let engine = state.engine.clone();

    tokio::task::spawn_blocking(move || {
        let prd_path = match req.prd_path {
            Some(ref p) => paths.root.join(p),
            None => paths.root.join("prd.json"),
        };

        // Resolve reference branch from config if not given.
        let reference_branch = req.reference_branch.clone().unwrap_or_else(|| {
            macc_core::load_canonical_config(&paths.config_path)
                .ok()
                .and_then(|c| c.automation.coordinator.and_then(|cc| cc.reference_branch))
                .unwrap_or_else(|| "main".to_string())
        });

        let request = PrdAuditRequest {
            prd_path,
            tool: req.tool,
            model_selection: req.model_selection.as_ref().map(to_model_selection),
            instructions: req.instructions,
            instructions_file: req.instructions_file.map(std::path::PathBuf::from),
            reference_branch,
            include_diff_stat: req.diff_stat,
            dry_run: req.dry_run,
        };

        let result = engine
            .prd_audit(&paths, &request)
            .map_err(|e| ApiError::validation(e.to_string()))?;

        Ok(Json(ApiPrdAuditResponse {
            completed_with_context: result.completed_with_context,
            todo_tasks: result.todo_tasks,
            prompt_generated: result.prompt_generated,
            prompt: result.prompt,
            dispatched: result.dispatched,
        }))
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))?
}

pub(crate) async fn prd_promote_handler(
    State(state): State<WebState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ApiPrdPromoteRequest>,
) -> std::result::Result<Json<PromoteResult>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let paths = state.paths.clone();
    let engine = state.engine.clone();

    tokio::task::spawn_blocking(move || {
        let source = paths.root.join(&req.source_path);
        let dest = req
            .dest_path
            .as_deref()
            .map(|p| paths.root.join(p))
            .unwrap_or_else(|| paths.root.join("prd.json"));

        let options = PromoteOptions {
            source_path: source,
            dest_path: dest,
            yes: req.yes,
            backup_dir: Some(paths.backups_dir.clone()),
        };

        engine
            .prd_promote(&options)
            .map(Json)
            .map_err(|e| ApiError::validation(e.to_string()))
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))?
}

pub(crate) async fn prd_validate_handler(
    State(state): State<WebState>,
    Json(req): Json<ApiPrdValidateRequest>,
) -> std::result::Result<Json<ValidationResult>, ApiError> {
    let paths = state.paths.clone();
    let engine = state.engine.clone();

    tokio::task::spawn_blocking(move || {
        let prd_path = paths.root.join(&req.prd_path);
        let result = engine.prd_validate(&prd_path, None);
        Ok::<_, ApiError>(Json(result))
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))?
}

pub(crate) async fn prd_generation_runs_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<Vec<ApiPrdRunEntry>>, ApiError> {
    let paths = state.paths.clone();
    let engine = state.engine.clone();

    tokio::task::spawn_blocking(move || {
        let runs = engine.prd_list_runs(&paths);
        Ok::<_, ApiError>(Json(
            runs.into_iter()
                .map(|(id, path)| ApiPrdRunEntry {
                    run_id: id,
                    path: path.to_string_lossy().to_string(),
                })
                .collect(),
        ))
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))?
}

pub(crate) async fn prd_generation_run_detail_handler(
    State(state): State<WebState>,
    Path(run_id): Path<String>,
) -> std::result::Result<Json<serde_json::Value>, ApiError> {
    let paths = state.paths.clone();

    tokio::task::spawn_blocking(move || {
        let run_dir = paths
            .root
            .join(PRD_GENERATION_DEFAULT_TARGET_DIR)
            .join(&run_id);

        if !run_dir.exists() {
            return Err(ApiError::not_found(
                format!("Run '{}' not found", run_id),
                None,
            ));
        }

        let metadata_path = run_dir.join("generation-metadata.json");
        let metadata: serde_json::Value = if metadata_path.exists() {
            let raw = std::fs::read_to_string(&metadata_path)
                .map_err(|e| ApiError::validation(e.to_string()))?;
            serde_json::from_str(&raw).unwrap_or(serde_json::json!({}))
        } else {
            serde_json::json!({})
        };

        let files: Vec<String> = std::fs::read_dir(&run_dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if p.is_file() {
                    Some(e.file_name().to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .collect();

        Ok(Json(serde_json::json!({
            "runId": run_id,
            "path": run_dir.to_string_lossy(),
            "files": files,
            "metadata": metadata,
        })))
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))?
}

// ── Helper ────────────────────────────────────────────────────────────────────

fn to_model_selection(api: &ApiModelSelection) -> ModelSelection {
    ModelSelection {
        mode: if api.mode == "manual" {
            ModelRoutingMode::Manual
        } else {
            ModelRoutingMode::Auto
        },
        model: api.model.clone(),
    }
}

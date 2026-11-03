use crate::commands::web::errors::ApiError;
use crate::commands::web::types::{ApiPrdResponse, ApiPrdTask, ApiPrdUpdateRequest};
use crate::commands::web::WebState;
use axum::{
    body::Bytes,
    extract::{Query, State},
    Json,
};
use macc_core::MaccError;
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
pub(crate) struct PrdQuery {
    path: Option<String>,
}

fn resolve_prd_path(state: &WebState, path_param: Option<&str>) -> Result<PathBuf, ApiError> {
    if path_param.is_none() && macc_core::coordinator::decisions::policy(&state.paths.root)?.enabled
    {
        let cfg = macc_core::load_canonical_config(&state.paths.config_path)?;
        return macc_core::prd_queue::active_path(
            &state.paths.root,
            cfg.automation.coordinator.as_ref(),
            None,
        )
        .map_err(ApiError::from);
    }
    let mut target = state.paths.root.clone();
    if let Some(p) = path_param {
        let p_path = Path::new(p);
        if p_path.is_absolute()
            || p_path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(
                MaccError::Validation("path parameter must be relative".to_string()).into(),
            );
        }
        target.push(p_path);
    }

    // If the path points to a directory, append prd.json or worktree.prd.json
    // But typical usage is either path to file or we just append prd.json.
    // The spec says "Support an optional ?path= query param for worktree-specific PRDs."
    // Let's assume the path param points to the file directly if it ends with .json,
    // otherwise append prd.json. Wait, it's safer to just check if it's a dir.
    // Actually, worktree PRD is usually named "worktree.prd.json" inside the worktree dir,
    // or just "prd.json" inside the project dir.
    // If we just use the provided path exactly when provided:
    if !target.to_string_lossy().ends_with(".json") {
        if target == state.paths.root {
            target.push("prd.json");
        } else {
            // For worktree path, usually we append worktree.prd.json
            target.push("worktree.prd.json");
        }
    }

    let root = state
        .paths
        .root
        .canonicalize()
        .map_err(|e| ApiError::validation(e.to_string()))?;
    let existing = if target.exists() {
        target.as_path()
    } else {
        target
            .parent()
            .ok_or_else(|| ApiError::validation("Invalid PRD path"))?
    };
    if !existing
        .canonicalize()
        .map_err(|e| ApiError::validation(e.to_string()))?
        .starts_with(&root)
    {
        return Err(ApiError::validation("PRD path escapes the project root"));
    }
    Ok(target)
}
fn authoritative(state: &WebState, path: &Path) -> Result<bool, ApiError> {
    let cfg = macc_core::load_canonical_config(&state.paths.config_path)?;
    let c = cfg.automation.coordinator.as_ref();
    let files = if let Some(c) = c.filter(|c| !c.prd_files.is_empty()) {
        c.prd_files.clone()
    } else {
        vec![c
            .and_then(|c| c.prd_file.clone())
            .unwrap_or_else(|| "prd.json".into())]
    };
    let path = path
        .canonicalize()
        .map_err(|e| ApiError::validation(e.to_string()))?;
    Ok(files.iter().any(|source| {
        state
            .paths
            .root
            .join(source)
            .canonicalize()
            .is_ok_and(|p| p == path)
    }))
}

fn decision_graph(root: &Path, prd_path: &Path) -> Result<(serde_json::Value, u64), ApiError> {
    let mut store = macc_core::coordinator::decisions::Store::open(root)?;
    store.project(root)?;
    let cfg =
        macc_core::load_canonical_config(&macc_core::ProjectPaths::from_root(root).config_path)?;
    let reference =
        macc_core::config::CoordinatorConfigResolved::resolve(cfg.automation.coordinator.as_ref())
            .reference_branch;
    let plan = store.ensure_plan(
        root,
        prd_path,
        &reference,
        &macc_core::coordinator::decisions::registry(root)?,
    )?;
    Ok((plan.graph, plan.graph_version))
}

pub(crate) async fn get_prd_handler(
    State(state): State<WebState>,
    Query(query): Query<PrdQuery>,
) -> Result<Json<ApiPrdResponse>, ApiError> {
    let prd_path = resolve_prd_path(&state, query.path.as_deref())?;

    let content = fs::read_to_string(&prd_path).map_err(|e| MaccError::Io {
        path: prd_path.to_string_lossy().to_string(),
        action: "read PRD".into(),
        source: e,
    })?;

    let mut parsed: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| MaccError::Validation(format!("Failed to parse PRD JSON: {}", e)))?;

    let graph_version = if macc_core::coordinator::decisions::policy(&state.paths.root)?.enabled
        && authoritative(&state, &prd_path)?
    {
        let (graph, version) = decision_graph(&state.paths.root, &prd_path)?;
        parsed = graph;
        Some(version)
    } else {
        None
    };
    let tasks_val = parsed
        .as_object_mut()
        .and_then(|obj| obj.remove("tasks"))
        .unwrap_or_else(|| serde_json::json!([]));
    let tasks = tasks_val
        .as_array()
        .ok_or_else(|| ApiError::validation("PRD tasks must be an array"))?
        .iter()
        .map(task_to_api)
        .collect::<Result<Vec<_>, _>>()?;
    let metadata: BTreeMap<String, serde_json::Value> = serde_json::from_value(parsed)
        .map_err(|e| MaccError::Validation(format!("Invalid PRD metadata format: {}", e)))?;

    Ok(Json(ApiPrdResponse {
        tasks,
        metadata,
        graph_version,
    }))
}

pub(crate) async fn update_prd_handler(
    State(state): State<WebState>,
    Query(query): Query<PrdQuery>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<Json<ApiPrdResponse>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let prd_path = resolve_prd_path(&state, query.path.as_deref())?;
    let payload: ApiPrdUpdateRequest = serde_json::from_slice(&body)
        .map_err(|err| MaccError::Validation(format!("Invalid PRD request body: {}", err)))?;

    // Validate JSON structure (unique task IDs, etc.)
    let mut seen_ids = HashSet::new();
    for task in &payload.tasks {
        if task.id.trim().is_empty() {
            return Err(MaccError::Validation("Task ID cannot be empty".to_string()).into());
        }
        if !seen_ids.insert(&task.id) {
            return Err(MaccError::Validation(format!("Duplicate task ID: {}", task.id)).into());
        }
    }

    // Reconstruct full JSON
    let mut full_json = serde_json::to_value(&payload.metadata)
        .map_err(|e| MaccError::Validation(format!("Failed to serialize metadata: {}", e)))?;

    let tasks_json = serde_json::Value::Array(payload.tasks.iter().map(task_from_api).collect());

    if let Some(obj) = full_json.as_object_mut() {
        obj.insert("tasks".to_string(), tasks_json);
    } else {
        return Err(MaccError::Validation("Metadata must be an object".to_string()).into());
    }

    if macc_core::coordinator::decisions::policy(&state.paths.root)?.enabled {
        if !authoritative(&state, &prd_path)? {
            return Err(ApiError::validation(
                "Worker PRDs are read-only projections; edit the configured authoritative plan",
            ));
        }
        let root = state.paths.root.clone();
        let version = payload.graph_version.ok_or_else(|| {
            ApiError::validation("A graph version is required; reload the PRD editor before saving")
        })?;
        let graph = tokio::task::spawn_blocking(move || -> macc_core::Result<serde_json::Value> {
            let cfg = macc_core::load_canonical_config(
                &macc_core::ProjectPaths::from_root(&root).config_path,
            )?;
            let reference = macc_core::config::CoordinatorConfigResolved::resolve(
                cfg.automation.coordinator.as_ref(),
            )
            .reference_branch;
            let mut store = macc_core::coordinator::decisions::Store::open(&root)?;
            let registry = macc_core::coordinator::decisions::registry(&root)?;
            let plan = store.ensure_plan(&root, &prd_path, &reference, &registry)?;
            let actor = macc_core::service::coordinator_workflow::operator_identity(&root);
            if actor.is_empty() {
                return Err(macc_core::coordinator::decisions::error(
                    "Configure the local operator identity before editing the plan",
                ));
            }
            let patch = store.edit_patch(&plan, &full_json, version, &actor)?;
            if !patch.operations.is_empty() || !patch.metadata_changes.is_empty() {
                store.apply_patch(&plan.id, &patch, &registry, &actor)?;
                store.project(&root)?;
            }
            Ok(serde_json::to_value(store.plan(&plan.id)?).unwrap())
        })
        .await
        .map_err(|e| ApiError::validation(e.to_string()))??;
        let mut canonical = graph["graph"].clone();
        let tasks = canonical.as_object_mut().unwrap().remove("tasks").unwrap();
        return Ok(Json(ApiPrdResponse {
            graph_version: graph["graph_version"].as_u64(),
            tasks: tasks
                .as_array()
                .unwrap()
                .iter()
                .map(task_to_api)
                .collect::<Result<Vec<_>, _>>()?,
            metadata: serde_json::from_value(canonical)
                .map_err(|e| ApiError::validation(e.to_string()))?,
        }));
    }
    let new_content = serde_json::to_string_pretty(&full_json)
        .map_err(|e| MaccError::Validation(format!("Failed to serialize new PRD: {}", e)))?;

    // Create backup
    if prd_path.exists() {
        let backups_dir = state.paths.root.join(".macc").join("backups");
        fs::create_dir_all(&backups_dir).map_err(|e| MaccError::Io {
            path: backups_dir.to_string_lossy().to_string(),
            action: "create backups directory".into(),
            source: e,
        })?;

        let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
        let backup_filename = format!(
            "{}.{}.bak",
            prd_path.file_name().unwrap_or_default().to_string_lossy(),
            timestamp
        );
        let backup_path = backups_dir.join(backup_filename);

        fs::copy(&prd_path, &backup_path).map_err(|e| MaccError::Io {
            path: prd_path.to_string_lossy().to_string(),
            action: "backup PRD file".into(),
            source: e,
        })?;
    } else {
        // Ensure parent directory exists for writing
        if let Some(parent) = prd_path.parent() {
            fs::create_dir_all(parent).map_err(|e| MaccError::Io {
                path: parent.to_string_lossy().to_string(),
                action: "create parent directory for PRD".into(),
                source: e,
            })?;
        }
    }

    // Write new content
    fs::write(&prd_path, new_content).map_err(|e| MaccError::Io {
        path: prd_path.to_string_lossy().to_string(),
        action: "write PRD file".into(),
        source: e,
    })?;

    Ok(Json(ApiPrdResponse {
        tasks: payload.tasks,
        metadata: payload.metadata,
        graph_version: None,
    }))
}

fn task_to_api(raw: &serde_json::Value) -> Result<ApiPrdTask, ApiError> {
    let mut object = raw
        .as_object()
        .ok_or_else(|| ApiError::validation("PRD task must be an object"))?
        .clone();
    for (from, to) in [
        ("base_branch", "baseBranch"),
        ("coordinator_tool", "coordinatorTool"),
        ("exclusive_resources", "exclusiveResources"),
    ] {
        if let Some(value) = object.remove(from) {
            object.insert(to.into(), value);
        }
    }
    let fields = [
        "id",
        "title",
        "priority",
        "category",
        "scope",
        "baseBranch",
        "coordinatorTool",
        "description",
        "objective",
        "result",
        "dependencies",
        "exclusiveResources",
        "steps",
        "notes",
    ];
    let mut extra = object
        .remove("metadata")
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    for key in object.keys().cloned().collect::<Vec<_>>() {
        if !fields.contains(&key.as_str()) {
            extra.insert(key.clone(), object.remove(&key).unwrap());
        }
    }
    object.insert("metadata".into(), serde_json::Value::Object(extra));
    serde_json::from_value(serde_json::Value::Object(object))
        .map_err(|e| ApiError::validation(e.to_string()))
}
fn task_from_api(task: &ApiPrdTask) -> serde_json::Value {
    let value = serde_json::to_value(task).unwrap();
    let mut object = task
        .metadata
        .clone()
        .into_iter()
        .collect::<serde_json::Map<_, _>>();
    for (key, value) in value.as_object().unwrap() {
        if key == "metadata" {
            continue;
        }
        let canonical = match key.as_str() {
            "baseBranch" => "base_branch",
            "coordinatorTool" => "coordinator_tool",
            "exclusiveResources" => "exclusive_resources",
            key => key,
        };
        if !value.is_null() {
            object.insert(canonical.into(), value.clone());
        } else {
            object.remove(canonical);
        }
    }
    serde_json::Value::Object(object)
}
#[cfg(test)]
mod decision_editor_tests {
    use super::*;
    #[test]
    fn editor_reads_committed_graph_even_before_prd_projection() {
        use macc_core::coordinator::decisions as dm;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        assert!(std::process::Command::new("git")
            .current_dir(root)
            .args(["init", "-q", "-b", "main"])
            .status()
            .unwrap()
            .success());
        std::fs::create_dir_all(root.join(".macc")).unwrap();
        let mut cfg = macc_core::config::CanonicalConfig::default();
        let c = cfg
            .automation
            .coordinator
            .get_or_insert_with(Default::default);
        c.prd_file = Some("prd.json".into());
        c.reference_branch = Some("main".into());
        c.decisions = Some(dm::Policy {
            enabled: true,
            ..Default::default()
        });
        std::fs::write(root.join(".macc/macc.yaml"), cfg.to_yaml().unwrap()).unwrap();
        std::fs::write(
            root.join("prd.json"),
            r#"{"tasks":[{"id":"A","dependencies":[]}]}"#,
        )
        .unwrap();
        let (mut store, plan) = dm::active_plan(root).unwrap();
        let patch:dm::Patch=serde_json::from_value(serde_json::json!({"plan_id":plan.id,"base_graph_version":1,"idempotency_key":"pending-export","reason":"Add dependent work","operations":[{"op":"add_task","task":{"id":"B","dependencies":["A"]}}]})).unwrap();
        store
            .apply_patch(&plan.id, &patch, &dm::registry(root).unwrap(), "Operator")
            .unwrap();
        assert!(!std::fs::read_to_string(root.join("prd.json"))
            .unwrap()
            .contains("\"B\""));
        let (graph, version) = decision_graph(root, &root.join("prd.json")).unwrap();
        assert_eq!(version, 2);
        assert!(graph["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == "B"));
    }
    #[test]
    fn round_trip_preserves_gate_scope_and_unknown_contract_fields() {
        let raw = serde_json::json!({"id":"A","base_branch":"main","exclusive_resources":["api"],"gate":{"kind":"human_approval"},"acceptance_criteria":["Exact criterion"],"spec_refs":["FR-1"]});
        let api = task_to_api(&raw).unwrap();
        let back = task_from_api(&api);
        for key in [
            "gate",
            "acceptance_criteria",
            "spec_refs",
            "base_branch",
            "exclusive_resources",
        ] {
            assert_eq!(back[key], raw[key]);
        }
    }
}

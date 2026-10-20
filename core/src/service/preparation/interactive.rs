//! CLI question loop over the same preparation report consumed by GUI clients.
use super::*;
use crate::service::lifecycle::LifecycleUi;
pub fn apply(
    paths: &ProjectPaths,
    engine: &dyn crate::Engine,
    initial: &mut crate::plan::ActionPlan,
    allow_user_scope: bool,
    ui: &dyn LifecycleUi,
    mut refresh: impl FnMut() -> Result<crate::plan::ActionPlan>,
) -> Result<crate::ApplyReport> {
    loop {
        let error = match with_progress(paths, engine, initial, allow_user_scope, ui) {
            Ok(report) => return Ok(report),
            Err(error) => error,
        };
        let report = status(&paths.root)?;
        let question = report.as_ref().and_then(|r| r.question.as_ref());
        if question.is_none() {
            ui.error(&error.to_string());
        }
        let mut inputs = initial
            .preparation
            .as_ref()
            .map(|p| p.inputs.clone())
            .unwrap_or_default();
        match question.map(|q| q.kind.as_str()) {
            Some("provider_quality") => {
                for warning in &report.as_ref().unwrap().warnings {
                    ui.warn(warning);
                }
                let question = question.unwrap();
                ui.decision_info(&question.message);
                ui.decision_info(&format!(
                    "Proposed setting: {} = {}",
                    question.data["path"].as_str().unwrap_or_default(),
                    question.data["value"].as_str().unwrap_or_default()
                ));
                let choice=ui.prompt_line("Partial index: [i] increase budget and rebuild, [k] keep partial coverage, [c] configure, [Enter] cancel: ")?;
                match choice.trim().to_ascii_lowercase().as_str() {
                    "i" => {
                        improve_quality(&paths.root, report.as_ref().unwrap(), true)?;
                        inputs.accept_degraded = false;
                    }
                    "k" => inputs.accept_degraded = true,
                    "c" => configure(&paths.root, ui)?,
                    _ => return Err(error),
                }
            }
            Some("network_access") => {
                if !ui.confirm_yes_no(
                    "Allow network access for project preparation and refresh Apply [y/N]? ",
                )? {
                    return Err(error);
                }
                allow_network(&paths.root, &report.as_ref().unwrap().revision, true)?;
            }
            Some("semantic_suite") => {
                ui.decision_info(&question.unwrap().message);
                let path = ui.prompt_line("Labelled benchmark JSON file (empty cancels): ")?;
                if path.trim().is_empty() {
                    return Err(error);
                }
                let bytes = std::fs::read(paths.root.join(path.trim())).map_err(|source| {
                    MaccError::Io {
                        path: path.trim().into(),
                        action: "read labelled suite".into(),
                        source,
                    }
                })?;
                if bytes.len() > 131072 {
                    return Err(MaccError::Validation(
                        "Benchmark input exceeds 128 KiB".into(),
                    ));
                }
                inputs.semantic_suite = Some(
                    serde_json::from_slice(&bytes)
                        .map_err(|e| MaccError::Validation(e.to_string()))?,
                );
                inputs.semantic_approval = None;
            }
            Some("semantic_review") => {
                let question = question.unwrap();
                ui.decision_info(
                    &serde_json::to_string_pretty(&question.data)
                        .map_err(|e| MaccError::Validation(e.to_string()))?,
                );
                if question.data["approvable"] != true
                    || !ui.confirm_yes_no(
                        "Approve these exact passing benchmark results and continue Apply [y/N]? ",
                    )?
                {
                    return Err(error);
                }
                inputs.semantic_approval = Some(Approval {
                    report_id: question.data["report"]["id"]
                        .as_str()
                        .unwrap_or_default()
                        .into(),
                    checksum: question.data["checksum"]
                        .as_str()
                        .unwrap_or_default()
                        .into(),
                });
                inputs.semantic_suite = None;
            }
            _ => {
                let answer = ui.prompt_line(
                    "Apply needs attention: [r] retry, [c] configure O2, [Enter] cancel: ",
                )?;
                match answer.trim().to_ascii_lowercase().as_str() {
                    "r" => {}
                    "c" => configure(&paths.root, ui)?,
                    _ => return Err(error),
                }
            }
        }
        *initial = refresh()?;
        set_inputs(initial, inputs);
        ui.decision_info(&crate::service::apply_presentation::review(
            paths,
            initial,
            &engine.plan_operations(paths, initial),
        ));
        if !ui.confirm_yes_no("Continue with the refreshed Apply plan [y/N]? ")? {
            return Err(MaccError::Validation(
                "Apply cancelled; completed preparation and evidence retained".into(),
            ));
        }
    }
}
fn with_progress(
    paths: &ProjectPaths,
    engine: &dyn crate::Engine,
    plan: &mut crate::plan::ActionPlan,
    allow_user_scope: bool,
    ui: &dyn LifecycleUi,
) -> Result<crate::ApplyReport> {
    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel();
        scope.spawn(move || loop {
            match rx.recv_timeout(std::time::Duration::from_millis(200)) {
                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
            if let Ok(Some(report)) = status(&paths.root) {
                ui.apply_progress(&report);
            }
        });
        let result = engine.apply(paths, plan, allow_user_scope);
        let _ = tx.send(());
        result
    })
}
fn configure(root: &Path, ui: &dyn LifecycleUi) -> Result<()> {
    let fields = crate::o2::policy::fields();
    for (i, field) in fields.iter().enumerate() {
        ui.decision_info(&format!("{}. {} ({})", i + 1, field.label, field.path));
    }
    let choice = ui.prompt_line("O2 setting number (empty cancels): ")?;
    let field = choice
        .trim()
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|i| fields.get(i));
    let Some(field) = field else {
        return Ok(());
    };
    let value = ui.prompt_line(&format!("{}: ", field.label))?;
    let preview = crate::o2::policy_edit::preview(root, &field.path, &value, None)?;
    ui.decision_info(
        &serde_json::to_string_pretty(&preview)
            .map_err(|e| MaccError::Validation(e.to_string()))?,
    );
    if ui.confirm_yes_no("Save this setting for Apply [y/N]? ")? {
        crate::o2::policy_edit::apply(root, &field.path, &value, &preview.revision, true)?;
    }
    Ok(())
}

//! Capture each attempt before migrations/files, retaining terminal evidence.
use super::*;
pub(crate) fn begin(paths: &ProjectPaths, plan: &Plan) -> Result<Report> {
    progress::clear(&paths.root)?;
    let report = Report {
        accepted_degraded: plan.inputs.accept_degraded,
        run_id: uuid::Uuid::new_v4().to_string(),
        output_kind: None,
        warnings: vec![],
        revision: plan.revision.clone(),
        status: "preparing".into(),
        completed: vec![],
        error: None,
        question: None,
        current: Some("Preparing configuration and O2 storage".into()),
        live_output: None,
    };
    publish(&paths.root, &report)?;
    Ok(report)
}
pub(crate) fn phase(root: &Path, message: &str) -> Result<()> {
    if let Some(mut report) = status(root)?.filter(|r| r.status == "preparing") {
        progress::clear(root)?;
        report.current = Some(message.into());
        report.live_output = None;
        publish(root, &report)?;
    }
    Ok(())
}
pub(crate) fn record_failure(root: &Path, error: &MaccError) -> Result<()> {
    if let Some(mut report) = status(root)?.filter(|r| r.status == "preparing") {
        report.status = "blocked".into();
        report.current = None;
        report.live_output = None;
        report.error = Some(error.to_string());
        publish(root, &report)?;
    }
    Ok(())
}

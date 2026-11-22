//! Capture each attempt before migrations/files, retaining terminal evidence.
use super::*;
pub(crate) fn begin(
    paths: &ProjectPaths,
    plan: &Plan,
    progress_token: Option<ProgressToken>,
) -> Result<Report> {
    progress::clear(&paths.root)?;
    let report = Report {
        progress_token,
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

impl Report {
    pub(crate) fn start_phase(&mut self, root: &Path, message: impl Into<String>) -> Result<()> {
        progress::clear(root)?;
        self.current = Some(message.into());
        publish(root, self)
    }
}

pub(crate) fn failure(root: &Path, mut report: Report, error: MaccError) -> MaccError {
    report.status = if report.question.is_some() {
        "waiting_for_input"
    } else {
        "blocked"
    }
    .into();
    report.current = None;
    report.live_output = None;
    report.error = Some(error.to_string());
    if let Err(err) = publish(root, &report) {
        tracing::warn!("Cannot record Apply failure: {err}");
    }
    MaccError::ApplyPreparation {
        report: Box::new(report),
        source: Box::new(error),
    }
}

pub fn live_status(root: &Path, token: ProgressToken) -> Result<Option<Report>> {
    Ok(status(root)?.filter(|r| r.progress_token == Some(token) && r.status == "preparing"))
}

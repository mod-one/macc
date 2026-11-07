//! A failure belongs to the current phase, not the previous dev explanation.
use crate::coordinator::{model::TaskRuntime, RuntimeStatus};
use serde_json::json;
pub(crate) fn record(runtime: &mut TaskRuntime, phase: &str, reason: &str) {
    runtime.extra.insert("previous_phase_outcome".into(),json!({"phase":runtime.current_phase,"completion_kind":runtime.completion_kind,"explanation":runtime.result_explanation,"last_error":runtime.last_error}));
    if let Some(previous) = runtime.extra.remove("diagnostic_delivery") {
        runtime
            .extra
            .insert("previous_diagnostic_delivery".into(), previous);
        runtime.message = None;
    }
    runtime.clear_last_error_details();
    runtime.result_explanation = None;
    runtime.unmet_preconditions.clear();
    runtime.completion_kind = None;
    runtime.set_status(RuntimeStatus::Failed);
    runtime.current_phase = Some(phase.into());
    runtime.last_error = Some(reason.into());
    let decision = crate::coordinator::phase_diagnostics::decision_precondition(reason);
    if reason.starts_with("E903:") || decision {
        runtime.set_last_error_details(
            "E903",
            if decision {
                "decisions"
            } else {
                "precondition"
            },
            reason,
        );
        runtime.completion_kind = Some("precondition_unmet".into());
        runtime.unmet_preconditions = vec![reason.into()];
    } else if reason.starts_with("E103:") || reason.contains("Missing exact MACC_DECISION_ACK") {
        runtime.set_last_error_details("E103", "output_protocol", reason);
    } else {
        runtime.set_last_error_details("E901", "phase", reason);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_tool_error_string_cannot_self_certify_native_diagnostic_delivery() {
        let mut runtime = TaskRuntime::default();
        record(
            &mut runtime,
            "review",
            r#"E903: DM_EXTERNAL_VERIFICATION: {"completed":true,"feature_accepted":false,"release_authorized":false,"unverified_decisions":["x"]}"#,
        );
        assert!(!runtime.extra.contains_key("diagnostic_delivery"));
    }

    #[test]
    fn review_error_replaces_the_active_dev_explanation_without_losing_history() {
        let mut r = TaskRuntime::default();
        r.result_explanation = Some("prior work unsalvageable: <reason>".into());
        r.completion_kind = Some("success_with_changes".into());
        r.current_phase = Some("dev".into());
        record(&mut r,"review","Decision control: DM_COMPLIANCE: decision X has no passing independent compliance evidence");
        assert_eq!(r.last_error_code.as_deref(), Some("E903"));
        assert_eq!(r.current_phase.as_deref(), Some("review"));
        assert!(r.result_explanation.is_none());
        assert_eq!(r.completion_kind.as_deref(), Some("precondition_unmet"));
        assert!(r.extra.contains_key("previous_phase_outcome"));
    }
}

/// Successful continuation clears active errors, retaining the prior diagnostic
/// as history rather than claiming the last failure still applies.
pub(super) fn clear(runtime: &mut TaskRuntime) {
    if let Some(value) = runtime.extra.remove("diagnostic_delivery") {
        runtime
            .extra
            .insert("previous_diagnostic_delivery".into(), value);
        runtime.message = None;
    }
    runtime.clear_last_error_details();
    runtime.last_error = None;
    runtime.unmet_preconditions.clear();
}

/// Called only with a native receipt loaded by the control plane.
pub(crate) fn attach(
    runtime: &mut TaskRuntime,
    delivery: crate::coordinator::decisions::DiagnosticDelivery,
) {
    let detail=format!("DM_EXTERNAL_VERIFICATION: Diagnostic report complete ({}); feature changes requested and release not authorized. Independent verification remains pending for {}. Complete the selected assistance or explicitly reconcile the decision; an unchanged retry cannot supply missing external evidence.",delivery.report_path,delivery.unverified_decisions.join(", "));
    runtime.last_error = Some(detail.clone());
    runtime.set_last_error_details("E903", "decisions", detail.clone());
    runtime.unmet_preconditions = vec![detail];
    runtime
        .extra
        .insert("diagnostic_delivery".into(), serde_json::json!(delivery));
    runtime.message=Some("Diagnostic report complete · feature changes requested · external verification pending; feature/release not accepted".into());
}

use crate::coordinator::error_normalizer::{
    error_code_to_canonical_class, CanonicalClass, ErrorNormalizer, ToolError,
};
use crate::coordinator::{
    resolve_completion_authority, CompletionAuthority, PerformerCompletionKind,
};

use super::JobCompletionInput;

#[derive(Debug, Clone)]
pub struct ErrorClassification {
    pub canonical_class: CanonicalClass,
    pub error_code: String,
    pub error_origin: String,
    pub error_message: String,
    pub completion_kind: Option<PerformerCompletionKind>,
    pub completion_success: bool,
    pub completion_authority: CompletionAuthority,
    pub has_commits: bool,
    pub tool_error: Option<ToolError>,
}

pub fn classify_completion_error(
    input: &JobCompletionInput,
    normalizer: Option<&dyn ErrorNormalizer>,
    has_commits: bool,
) -> ErrorClassification {
    // ── Baseline error classification from caller ────────────────────
    let raw_error_code = input
        .error_code
        .clone()
        .unwrap_or_else(|| "E101".to_string());
    let error_origin = input
        .error_origin
        .clone()
        .unwrap_or_else(|| "runner".to_string());
    let raw_error_message = input
        .error_message
        .clone()
        .unwrap_or_else(|| input.status_text.clone());

    // ── Per-adapter error normalization ──────────────────────────────
    // Run when the caller provides raw process output AND the job failed.
    // The normalizer output takes priority over the caller-supplied error code.
    // A structured precondition report is the tool's own account of why it
    // stopped; matching its log against provider error patterns can only
    // misclassify it (a summary mentioning "usage limit" is not a quota).
    let reported_preconditions = input.completion_kind
        == Some(PerformerCompletionKind::PreconditionUnmet)
        || input
            .unmet_preconditions
            .iter()
            .any(|p| !p.trim().is_empty());
    let protocol_failure =
        raw_error_code == "E103" && matches!(error_origin.as_str(), "output_protocol" | "runner");
    let tool_error: Option<ToolError> =
        if !input.success && !reported_preconditions && !protocol_failure {
            input.normalizer_input.as_ref().and_then(|ni| {
                let stderr = crate::coordinator::result_protocol::provider_output(&ni.stderr);
                let stdout = crate::coordinator::result_protocol::provider_output(&ni.stdout);
                if stderr.is_empty() && stdout.is_empty() {
                    return None;
                }
                normalizer.and_then(|n| {
                    n.normalize(ni.exit_code, &stderr, &stdout).map(|mut te| {
                        te.attempt = input.attempt as u32;
                        te.operation = "performer_run".into();
                        te
                    })
                })
            })
        } else {
            None
        };

    // Override caller-supplied error code/message with normalizer output.
    let error_code = if reported_preconditions {
        "E903".to_string()
    } else {
        tool_error
            .as_ref()
            .map(|te| te.error_code.clone())
            .unwrap_or(raw_error_code)
    };
    // The structured report outranks normalized log text, which is only an
    // excerpt of the performer log.
    let error_message = if protocol_failure {
        Some(raw_error_message.clone())
    } else {
        structured_error_message(input)
    }
    .or_else(|| tool_error.as_ref().map(|te| te.raw_message.clone()))
    .unwrap_or(raw_error_message);
    let canonical_class = tool_error
        .as_ref()
        .map(|te| te.canonical_class.clone())
        .unwrap_or_else(|| error_code_to_canonical_class(&error_code));

    // Resolve authority last, after normalization is complete.
    let completion_resolution =
        resolve_completion_authority(input.completion_kind, has_commits, input.success);
    let completion_success = if input.completion_kind.is_some() {
        completion_resolution.success
    } else {
        input.success
    };
    let completion_kind = input.completion_kind;

    ErrorClassification {
        canonical_class,
        error_code,
        error_origin: if reported_preconditions {
            "precondition".into()
        } else {
            error_origin
        },
        error_message,
        completion_kind,
        completion_success,
        completion_authority: completion_resolution.authority,
        has_commits,
        tool_error,
    }
}

/// The tool's own account of a failure: its unmet preconditions, else its
/// `MACC_TASK_RESULT_EXP` explanation.
fn structured_error_message(input: &JobCompletionInput) -> Option<String> {
    let preconditions: Vec<&str> = input
        .unmet_preconditions
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    let explanation = input
        .result_explanation
        .as_deref()
        .map(str::trim)
        .filter(|e| !e.is_empty());
    if !preconditions.is_empty() {
        let mut message = format!(
            "unmet precondition{}: {}",
            if preconditions.len() == 1 { "" } else { "s" },
            preconditions.join("; ")
        );
        if let Some(explanation) = explanation {
            message.push_str(" — ");
            message.push_str(explanation);
        }
        return Some(message);
    }
    explanation.map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator::engine::NormalizerInput;
    struct MustNotNormalize;
    impl ErrorNormalizer for MustNotNormalize {
        fn normalize(&self, _: i32, _: &str, _: &str) -> Option<ToolError> {
            panic!("structured preconditions must not be replaced by tool log matches")
        }
    }
    #[test]
    fn precondition_explanation_survives_misleading_auth_text_in_diff() {
        let input = JobCompletionInput {
            success: false,
            attempt: 1,
            max_attempts: 3,
            timed_out: false,
            phase_timeout_seconds: 3600,
            elapsed_seconds: 1,
            status_text: "precondition missing".into(),
            completion_kind: Some(PerformerCompletionKind::PreconditionUnmet),
            error_code: Some("E601".into()),
            error_origin: Some("precondition".into()),
            error_message: Some("authorized schema migration needed".into()),
            result_explanation: Some("schema change required".into()),
            unmet_preconditions: vec!["response snapshots".into()],
            auto_retry_error_codes: vec![],
            auto_retry_max: 0,
            backoff_base_seconds: 30,
            backoff_max_seconds: 300,
            normalizer_input: Some(NormalizerInput {
                exit_code: 1,
                stderr: String::new(),
                stdout: "old quota exceeded; diff --git: invalid_api_key permission_denied".into(),
            }),
        };
        let result = classify_completion_error(&input, Some(&MustNotNormalize), false);
        assert_eq!(result.error_code, "E903");
        assert_eq!(
            result.error_message,
            "unmet precondition: response snapshots — schema change required"
        );
        assert_eq!(
            result.completion_kind,
            Some(PerformerCompletionKind::PreconditionUnmet)
        );
        assert!(result.tool_error.is_none());
    }

    struct QuotaFromLog;
    impl ErrorNormalizer for QuotaFromLog {
        fn normalize(&self, _: i32, _: &str, stdout: &str) -> Option<ToolError> {
            Some(ToolError {
                provider: "claude".into(),
                canonical_class:
                    crate::coordinator::error_normalizer::CanonicalClass::QuotaExhausted,
                raw_message: stdout.chars().take(40).collect(),
                error_code: "E602".into(),
                retryable: true,
                user_action_required: false,
                retry_after_seconds: None,
                request_id: None,
                attempt: 0,
                operation: String::new(),
            })
        }
    }

    fn failed_input(kind: Option<PerformerCompletionKind>) -> JobCompletionInput {
        JobCompletionInput {
            success: false,
            attempt: 1,
            max_attempts: 3,
            timed_out: false,
            phase_timeout_seconds: 3600,
            elapsed_seconds: 1,
            status_text: "performer failed".into(),
            completion_kind: kind,
            error_code: None,
            error_origin: None,
            error_message: None,
            result_explanation: None,
            unmet_preconditions: vec![],
            auto_retry_error_codes: vec![],
            auto_retry_max: 0,
            backoff_base_seconds: 30,
            backoff_max_seconds: 300,
            normalizer_input: Some(NormalizerInput {
                exit_code: 1,
                stderr: String::new(),
                stdout: "ERROR: provider usage limit reached".into(),
            }),
        }
    }

    #[test]
    fn listed_preconditions_are_never_reclassified_from_the_log() {
        let mut input = failed_input(Some(PerformerCompletionKind::ErrorWithoutChanges));
        input.unmet_preconditions = vec!["API audience must include web".into()];
        input.result_explanation = Some("the BFF route is not exposed to the web audience".into());
        let result = classify_completion_error(&input, Some(&QuotaFromLog), false);
        assert!(result.tool_error.is_none());
        assert_ne!(result.error_code, "E602");
        assert_eq!(
            result.error_message,
            "unmet precondition: API audience must include web — the BFF route is not exposed to the web audience"
        );
    }

    #[test]
    fn the_tool_explanation_outranks_the_normalized_log_excerpt() {
        let mut input = failed_input(Some(PerformerCompletionKind::ErrorWithoutChanges));
        input.result_explanation = Some("build failed on missing contract".into());
        let result = classify_completion_error(&input, Some(&QuotaFromLog), false);
        // The class still comes from the normalizer; the message from the tool.
        assert_eq!(result.error_code, "E602");
        assert_eq!(result.error_message, "build failed on missing contract");
    }

    #[test]
    fn without_a_structured_report_the_normalized_message_is_kept() {
        let input = failed_input(None);
        let result = classify_completion_error(&input, Some(&QuotaFromLog), false);
        assert_eq!(result.error_code, "E602");
        assert!(result
            .error_message
            .starts_with("ERROR: provider usage limit"));
    }
    #[test]
    fn native_protocol_diagnostic_outranks_fabricated_tool_failure_and_log_matches() {
        let mut input = failed_input(Some(PerformerCompletionKind::ErrorWithoutChanges));
        input.error_code = Some("E103".into());
        input.error_origin = Some("output_protocol".into());
        input.error_message =
            Some("Missing explicit MACC_TASK_RESULT; inspect raw transcript".into());
        input.result_explanation = Some("no explanation provided".into());
        let result = classify_completion_error(&input, Some(&MustNotNormalize), true);
        assert_eq!(result.error_code, "E103");
        assert_eq!(result.error_message, input.error_message.unwrap());
        assert!(result.tool_error.is_none());
        assert!(!result.completion_success);
    }
}

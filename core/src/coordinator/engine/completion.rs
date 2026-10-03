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
    let tool_error: Option<ToolError> = if !input.success
        && input.completion_kind != Some(PerformerCompletionKind::PreconditionUnmet)
    {
        input.normalizer_input.as_ref().and_then(|ni| {
            normalizer.and_then(|n| {
                n.normalize(ni.exit_code, &ni.stderr, &ni.stdout)
                    .map(|mut te| {
                        te.attempt = input.attempt as u32;
                        te.operation = "performer_run".to_string();
                        te
                    })
            })
        })
    } else {
        None
    };

    // Override caller-supplied error code/message with normalizer output.
    let error_code = tool_error
        .as_ref()
        .map(|te| te.error_code.clone())
        .unwrap_or(raw_error_code);
    let error_message = tool_error
        .as_ref()
        .map(|te| te.raw_message.clone())
        .unwrap_or(raw_error_message);
    let canonical_class = tool_error
        .as_ref()
        .map(|te| te.canonical_class.clone())
        .unwrap_or_else(|| error_code_to_canonical_class(&error_code));

    // Resolve authority last, after normalization is complete.
    let completion_resolution =
        resolve_completion_authority(input.completion_kind, has_commits, input.success);
    let ipc_phase_done_override = completion_resolution.authority == CompletionAuthority::IpcSignal
        && completion_resolution.completion_kind
            == Some(PerformerCompletionKind::SuccessWithChanges);
    let completion_success = if ipc_phase_done_override {
        true
    } else {
        input.success
    };
    let completion_kind = if ipc_phase_done_override {
        Some(PerformerCompletionKind::SuccessWithChanges)
    } else {
        input.completion_kind
    };

    ErrorClassification {
        canonical_class,
        error_code,
        error_origin,
        error_message,
        completion_kind,
        completion_success,
        completion_authority: completion_resolution.authority,
        has_commits,
        tool_error,
    }
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
            error_code: Some("E903".into()),
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
                stdout: "diff --git: invalid_api_key permission_denied".into(),
            }),
        };
        let result = classify_completion_error(&input, Some(&MustNotNormalize), false);
        assert_eq!(result.error_code, "E903");
        assert_eq!(result.error_message, "authorized schema migration needed");
        assert_eq!(
            result.completion_kind,
            Some(PerformerCompletionKind::PreconditionUnmet)
        );
        assert!(result.tool_error.is_none());
    }
}

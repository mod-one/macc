//! Explicit operator response semantics, independent from UI and provider.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const RESPONSE_MODE: &str = "intervention_response";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionResponse {
    Completed,
    ConfirmFact,
    DeferObservation,
    Unable,
}
impl InterventionResponse {
    pub fn value(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::ConfirmFact => "confirm_fact",
            Self::DeferObservation => "defer_observation",
            Self::Unable => "unable",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Completed => "Action completed — report the result",
            Self::ConfirmFact => "Confirm a fact (operator statement)",
            Self::DeferObservation => "Explicitly defer the observation; keep it unverified",
            Self::Unable => "Unable to help — keep dependent work stopped",
        }
    }
}
pub fn intervention_response(
    request: &Request,
    option: Option<&str>,
    fields: &BTreeMap<String, String>,
) -> crate::Result<InterventionResponse> {
    let declared = request
        .options
        .iter()
        .find(|o| Some(o.id.as_str()) == option)
        .and_then(|o| o.intervention_response);
    let explicit = fields
        .get(RESPONSE_MODE)
        .map(|v| {
            serde_json::from_value::<InterventionResponse>(serde_json::json!(v))
                .map_err(|_| error("Unknown intervention response type"))
        })
        .transpose()?;
    if declared.is_some() && explicit.is_some() && declared != explicit {
        return Err(error(
            "Response type contradicts the selected option; refresh the response",
        ));
    }
    Ok(declared
        .or(explicit)
        .unwrap_or(InterventionResponse::Completed))
}
pub fn validate_intervention(request: &Request, input: &AnswerInput) -> crate::Result<()> {
    if request.kind != "intervention" {
        if input.fields.contains_key(RESPONSE_MODE) {
            return Err(error(
                "Intervention response type requires an intervention request",
            ));
        }
        return Ok(());
    }
    let mode = intervention_response(request, input.option.as_deref(), &input.fields)?;
    if input.outcome != AnswerOutcome::Answer {
        return Ok(());
    }
    if input.advisory {
        return Err(error(
            "Intervention responses must be attributable binding answers",
        ));
    }
    match mode {
        InterventionResponse::Completed => {
            if input.fields.get("performed").map(String::as_str) != Some("yes")
                || input
                    .fields
                    .get("result")
                    .is_none_or(|v| v.trim().is_empty())
            {
                return Err(error("Explicit confirmation performed=yes and the actual observed result are required; no physical action is inferred"));
            }
        }
        InterventionResponse::ConfirmFact | InterventionResponse::DeferObservation => {
            if input.fields.get("performed").is_some_and(|v| v == "yes") {
                return Err(error("A fact confirmation or deferral must not claim a physical action was performed"));
            }
            if input.apply_patch {
                return Err(error(
                    "Confirming or deferring an observation does not authorize graph changes",
                ));
            }
        }
        InterventionResponse::Unable => {
            return Err(error(
                "Use outcome reject for an inability to perform the intervention",
            ))
        }
    }
    Ok(())
}

pub fn field_required(
    request: &Request,
    input: &AnswerInput,
    field: &ResponseField,
) -> crate::Result<bool> {
    if !field.required || input.outcome != AnswerOutcome::Answer {
        return Ok(false);
    }
    if request.kind == "intervention" {
        let mode = intervention_response(request, input.option.as_deref(), &input.fields)?;
        if mode == InterventionResponse::DeferObservation || mode == InterventionResponse::Unable {
            return Ok(false);
        }
        if mode == InterventionResponse::ConfirmFact
            && matches!(field.id.as_str(), "performed" | "result")
        {
            return Ok(false);
        }
    }
    Ok(true)
}

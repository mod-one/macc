//! Adapter-owned preparation quality, independent from runtime availability.
use super::*;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Quality {
    pub warnings: Vec<String>,
    pub suggestion: Option<SettingSuggestion>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingSuggestion {
    pub path: String,
    pub value: String,
}
pub fn inspect(root: &Path, id: &str) -> Result<Quality> {
    let policy = super::super::policy::load(root)?.code_intelligence;
    let (db, _) = readonly_database(root)?;
    let state = receipt(&db, id)?.ok_or_else(|| error("Provider receipt missing"))?;
    Ok(provider(id)?.preparation_quality(&state, &policy))
}
pub(super) fn native(state: &Receipt, policy: &super::super::policy::CodePolicy) -> Quality {
    let Some(metadata) = &state.metadata else {
        return Quality::default();
    };
    let partial = metadata["truncated"] == true;
    let failures = metadata["stats"]["failed_files"].as_u64().unwrap_or(0);
    let mut quality = Quality::default();
    if partial {
        quality.warnings.push(format!("Native index has partial coverage ({} files parsed, {} ms elapsed). Increase the build budget or retain partial coverage; details are in the provider receipt.", metadata["stats"]["parsed_files"].as_u64().unwrap_or(0), metadata["stats"]["elapsed_ms"].as_u64().unwrap_or(0)));
        if metadata["stats"]["elapsed_ms"].as_u64().unwrap_or(0) >= policy.native_build_timeout_ms
            && policy.native_build_timeout_ms < 60000
        {
            quality.suggestion = Some(SettingSuggestion {
                path: "code_intelligence.native_build_timeout_ms".into(),
                value: policy
                    .native_build_timeout_ms
                    .saturating_mul(3)
                    .min(60000)
                    .to_string(),
            });
        }
    }
    if failures > 0 {
        quality.warnings.push(format!("Native index: {failures} files could not be parsed. Their symbol results are unavailable; inspect provider diagnostics. A larger build budget does not fix source parse errors."));
    }
    quality
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_and_parse_errors_have_distinct_remediation() {
        let mut state: Receipt=serde_json::from_value(serde_json::json!({"provider_id":"native","version":"test","state":"healthy","owned_paths":[],"metadata":{"truncated":true,"stats":{"parsed_files":1160,"failed_files":9,"elapsed_ms":5020}}})).unwrap();
        let policy = super::super::super::policy::CodePolicy {
            native_build_timeout_ms: 5000,
            ..Default::default()
        };
        let quality = native(&state, &policy);
        assert_eq!(quality.warnings.len(), 2);
        assert_eq!(quality.suggestion.unwrap().value, "15000");
        state.metadata.as_mut().unwrap()["truncated"] = serde_json::json!(false);
        let quality = native(&state, &policy);
        assert_eq!(quality.warnings.len(), 1);
        assert!(quality.suggestion.is_none());
    }
}

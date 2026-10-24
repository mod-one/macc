//! Supervisor configuration readiness and contract regression coverage.
use super::*;

pub(super) fn readiness(
    paths: &ProjectPaths,
    config: &macc_core::supervisor::SupervisorConfig,
) -> Vec<String> {
    if !config.repair_enabled {
        return vec!["Diagnosis only: automatic repair is disabled".into()];
    }
    match macc_core::supervisor::validation::commands(&paths.root, config) {
        Ok(commands) if commands.is_empty() => vec![if config.validation_commands.is_empty() {
            "Tests and checks are disabled: validation_commands is empty. Add a command to enable tests.".into()
        } else { "Tests and checks are disabled".into() }],
        Ok(commands) => commands.into_iter().map(|c| format!("Check: {c}")).collect(),
        Err(error) => vec![format!("Repair not ready: {error}. Configure relative subproject commands or explicitly disable tests.")],
    }
}

#[cfg(test)]
mod supervisor_tests {
    use super::*;
    #[test]
    fn config_api_exposes_and_persists_supervisor_without_changing_coordinator() {
        let mut canonical = CanonicalConfig::default();
        let before = canonical.automation.coordinator.clone();
        let mut response = ApiConfigResponse::from(canonical.clone());
        assert!(!response.supervisor.tests_enabled);
        assert_eq!(
            response.supervisor_fields.len(),
            macc_core::supervisor::settings::FIELDS.len()
        );
        response.supervisor.tests_enabled = false;
        response.supervisor.tool = Some("custom-tool".into());
        let request = ApiConfigUpdateRequest {
            supervisor: Some(serde_json::to_value(response.supervisor.clone()).unwrap()),
            ..Default::default()
        };
        apply_update(&mut canonical, &request, &[]).unwrap();
        canonical.validate().unwrap();
        assert_eq!(canonical.automation.coordinator, before);
        assert_eq!(canonical.automation.supervisor, Some(response.supervisor));
        let request: ApiConfigUpdateRequest =
            serde_json::from_value(serde_json::json!({"supervisor":{"max_repair_rounds":0}}))
                .unwrap();
        assert!(apply_update(&mut canonical, &request, &[]).is_err());
        let partial: ApiConfigUpdateRequest = serde_json::from_value(
            serde_json::json!({"supervisor":{"ai_verification_enabled":false}}),
        )
        .unwrap();
        apply_update(&mut canonical, &partial, &[]).unwrap();
        let config = canonical.automation.supervisor.unwrap();
        assert_eq!(config.tool.as_deref(), Some("custom-tool"));
        assert!(!config.tests_enabled && !config.ai_verification_enabled);
    }
    #[test]
    fn monorepo_readiness_explains_missing_commands_and_explicit_opt_out() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        let mut config = macc_core::supervisor::SupervisorConfig::default();
        assert!(readiness(&paths, &config)[0].contains("validation_commands is empty"));
        config.tests_enabled = true;
        assert!(readiness(&paths, &config)[0].contains("Repair not ready"));
        config.tests_enabled = false;
        assert!(readiness(&paths, &config)[0].contains("validation_commands is empty"));
    }

    #[test]
    fn api_command_edits_toggle_tests_and_reject_activation_without_commands() {
        let mut config = CanonicalConfig::default();
        let request = |value| ApiConfigUpdateRequest {
            supervisor: Some(value),
            ..Default::default()
        };
        apply_update(
            &mut config,
            &request(serde_json::json!({"validation_commands":["npm test"]})),
            &[],
        )
        .unwrap();
        assert!(config.automation.supervisor.as_ref().unwrap().tests_enabled);
        apply_update(
            &mut config,
            &request(serde_json::json!({"validation_commands":[]})),
            &[],
        )
        .unwrap();
        assert!(!config.automation.supervisor.as_ref().unwrap().tests_enabled);
        let before = config.clone();
        let error = apply_update(
            &mut config,
            &request(serde_json::json!({"tests_enabled":true})),
            &[],
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("Add at least one validation command"));
        assert_eq!(config, before);
    }
}

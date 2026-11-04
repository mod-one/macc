//! Decision policy drafts in the unified configuration workspace.
use super::*;
use macc_core::coordinator::decisions as dm;

pub(super) struct Field {
    pub label: &'static str,
    key: &'static str,
    choices: &'static [&'static str],
    kind: &'static str,
    pub(super) help: &'static str,
}
macro_rules! field {
    ($label:expr,$key:expr,$kind:expr,$choices:expr,$help:expr) => {
        Field {
            label: $label,
            key: $key,
            kind: $kind,
            choices: $choices,
            help: $help,
        }
    };
}
pub(super) const FIELDS: &[Field] = &[
 field!("Enabled","enabled","bool",&[],"Enable durable decision management. First activation and disabling require a stopped coordinator."),
 field!("Human question threshold","human_question_min_risk","enum",&["low","medium","high","critical","never_ask"],"Ask only important unresolved choices at or above this risk. Higher thresholds grant more autonomy. never_ask records delegated choices; unavailable essential capabilities remain blocked without invented results."),
 field!("Context byte budget","max_context_bytes","number",&[],"2048–262144 bytes. Mandatory decisions are never silently truncated."),
 field!("Request byte budget","max_request_bytes","number",&[],"2048–262144 bytes per request, including structured evidence references."),
 field!("Mandatory human triggers","mandatory_human_triggers","list",&[],"Comma-separated triggers, or a JSON string array. Native policy derives risk and routing."),
 field!("Approval reuse","approval_reuse","enum",&["exact_revision","content_equivalent","never"],"Reuse only under current verified subject, policy and role checks. Content-equivalent reuse requires explicit manifests."),
 field!("Trust mode","trust_mode","enum",&["local_trust","secure_approval"],"Local trust attributes the operator locally. Secure approval requires an independently administered registry; policy changes then require isolated approval."),
 field!("Trust registry","trust_registry","text",&[],"Path to the independent protected trust registry. Empty clears the optional path."),
 field!("Investigation rounds","investigation.max_rounds","number",&[],"1–10 bounded planning/investigation attempts per request."),
 field!("Investigation source bytes","investigation.max_source_bytes","number",&[],"1024–262144 source bytes for bounded investigation."),
 field!("Similarity suggestions","similarity_hints","bool",&[],"Advisory suggestions only; they never merge authority or create consent."),
 field!("Similarity backend","similarity_backend","enum",&["lexical","local_lsa"],"Local lexical or TF-IDF/LSA hints; no extra paid provider call."),
 field!("Similarity candidates","similarity_max_candidates","number",&[],"1–512 candidate requests."),
 field!("Similarity minimum percent","similarity_min_percent","number",&[],"1–100 percent matching threshold for advisory suggestions."),
 field!("Governance rules (JSON)","governance_rules","json",&[],"Advanced rule array: scope, minimum risk, standing directions and required forms. Existing rules are preserved until explicitly edited."),
 field!("Approval manifests (JSON)","approval_manifests","json",&[],"Object mapping gate IDs to explicit repository-relative source files."),
 field!("Operation adapters (JSON)","operation_adapters","json",&[],"Trusted native operation adapters. Configure explicit executables, targets and bounded execution; a reservation alone is not completion."),
 field!("Approver attestation (advanced)","proof","proof",&[],"Optional isolated attestation JSON for this exact policy change; never saved in macc.yaml. F12 exports its challenge. A configured broker can also approve the save through Decisions > Actions > Independent approval requests."),
];
impl AppState {
    pub(super) fn decision_policy(&self) -> dm::Policy {
        policy_from(self.working_copy.as_ref())
    }
    pub fn decision_field_value(&self, index: usize) -> String {
        let Some(field) = FIELDS.get(index) else {
            return String::new();
        };
        if field.key == "proof" {
            return if self.decision_settings_proof.is_empty() {
                String::new()
            } else {
                "Provided (not saved)".into()
            };
        }
        let policy = self.decision_policy();
        let value = serde_json::to_value(&policy).unwrap_or_default();
        let value = field.key.split('.').fold(&value, |v, key| &v[key]);
        if value.is_null() {
            return match field.key {
                "similarity_hints" => policy.similarity_hints.to_string(),
                "similarity_backend" => policy.similarity_backend,
                "similarity_max_candidates" => policy.similarity_max_candidates.to_string(),
                "similarity_min_percent" => policy.similarity_min_percent.to_string(),
                "investigation.max_rounds" => policy.investigation.max_rounds.to_string(),
                "investigation.max_source_bytes" => {
                    policy.investigation.max_source_bytes.to_string()
                }
                "governance_rules" => "[]".into(),
                "operation_adapters" => "{}".into(),
                _ => String::new(),
            };
        }
        match value {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            v => v.to_string(),
        }
    }
    pub fn decision_field_info(&self, index: usize) -> String {
        let field = &FIELDS[index];
        let mut text=format!("{}\n\n{}\n\nValue: {}\n\nEnter/Space: toggle, cycle or edit. Left/Right: adjust. e: exact entry.\ns: save configuration. F12: isolated approval challenge.\nQuestions, answers and history remain in the Decisions center (D).",field.label,field.help,self.decision_field_value(index));
        if let Err(e) = self.decision_policy().validate() {
            text.push_str(&format!("\n\nDraft validation: {e}"));
        }
        text
    }
    pub fn edit_decision_field(&mut self, index: usize, toggle: bool) {
        let Some(field) = FIELDS.get(index) else {
            return;
        };
        if toggle && (field.kind == "bool" || !field.choices.is_empty()) {
            self.adjust_decision_field(index, true);
            return;
        }
        self.settings_field_input = if field.key == "proof" {
            self.decision_settings_proof.clone()
        } else {
            self.decision_field_value(index)
        };
        self.settings_field_editing = true;
    }
    pub fn adjust_decision_field(&mut self, index: usize, forward: bool) {
        let Some(field) = FIELDS.get(index) else {
            return;
        };
        let current = self.decision_field_value(index);
        let next = if field.kind == "bool" {
            (current != "true").to_string()
        } else if !field.choices.is_empty() {
            let at = field
                .choices
                .iter()
                .position(|s| *s == current)
                .unwrap_or(0);
            field.choices
                [(at + if forward { 1 } else { field.choices.len() - 1 }) % field.choices.len()]
            .into()
        } else if field.kind == "number" {
            let n = current.parse::<u64>().unwrap_or(0);
            if forward {
                n.saturating_add(1)
            } else {
                n.saturating_sub(1)
            }
            .to_string()
        } else {
            self.edit_decision_field(index, false);
            return;
        };
        self.set_decision_field(index, &next);
    }
    fn set_decision_field(&mut self, index: usize, input: &str) -> bool {
        let field = &FIELDS[index];
        let result = (|| -> macc_core::Result<()> {
            if field.key == "proof" {
                if !input.trim().is_empty() {
                    serde_json::from_str::<dm::Attestation>(input)
                        .map_err(|e| dm::error(e.to_string()))?;
                }
                self.decision_settings_proof = input.into();
                return Ok(());
            }
            let value = match field.kind {
                "bool" => serde_json::from_str::<bool>(input)
                    .map(Value::Bool)
                    .map_err(|e| dm::error(e.to_string()))?,
                "number" => {
                    Value::from(input.parse::<u64>().map_err(|e| dm::error(e.to_string()))?)
                }
                "json" => serde_json::from_str(input).map_err(|e| dm::error(e.to_string()))?,
                "list" if input.trim().starts_with('[') => {
                    serde_json::from_str(input).map_err(|e| dm::error(e.to_string()))?
                }
                "list" => serde_json::json!(input
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()),
                _ if field.key == "trust_registry" && input.trim().is_empty() => Value::Null,
                _ => Value::String(input.to_string()),
            };
            let mut draft = serde_json::to_value(self.decision_policy())
                .map_err(|e| dm::error(e.to_string()))?;
            if let Some((parent, key)) = field.key.split_once('.') {
                if draft.get(parent).is_none() {
                    draft[parent] = serde_json::json!({});
                }
                draft[parent][key] = value;
            } else {
                draft[field.key] = value;
            }
            let policy: dm::Policy =
                serde_json::from_value(draft).map_err(|e| dm::error(e.to_string()))?;
            self.snapshot_before_config_change();
            if let Some(c) = &mut self.working_copy {
                c.automation
                    .coordinator
                    .get_or_insert_with(Default::default)
                    .decisions = Some(policy);
            }
            self.decision_settings_proof.clear();
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.set_status(
                    UiStatusLevel::Info,
                    "Decisions draft updated. Press s to save.",
                    None,
                );
                true
            }
            Err(e) => {
                self.set_status(UiStatusLevel::Error, e.to_string(), None);
                false
            }
        }
    }
    pub fn commit_decision_edit(&mut self, index: usize) {
        let input = self.settings_field_input.clone();
        if self.set_decision_field(index, &input) {
            self.settings_field_editing = false;
        }
    }
    pub fn decision_settings_challenge(&mut self) {
        let Some(paths) = &self.project_paths else {
            return;
        };
        self.decision_center.submit(
            paths.root.clone(),
            self.client_context.client_id.clone(),
            crate::decision_center::Action::Operator {
                intent: dm::OperatorIntent::Configure {
                    policy: self.decision_policy(),
                },
                proof: None,
                challenge: true,
            },
        );
        self.decision_center.show();
    }
    /// Persist policy through native guards before the ordinary whole-config save.
    pub(super) fn persist_decision_settings(&mut self) -> macc_core::Result<bool> {
        let updated = self.decision_policy();
        let previous = policy_from(self.config.as_ref());
        if updated == previous {
            return Ok(true);
        }
        updated.validate()?;
        let paths = self
            .project_paths
            .as_ref()
            .ok_or_else(|| dm::error("No project loaded"))?
            .clone();
        let disk = macc_core::load_canonical_config(&paths.config_path)?;
        let current = policy_from(Some(&disk));
        // Only an independently approved exact policy may account for a changed base.
        let mut comparable = disk.clone();
        if let Some(c) = &mut comparable.automation.coordinator {
            c.decisions = self
                .config
                .as_ref()
                .and_then(|x| x.automation.coordinator.as_ref())
                .and_then(|x| x.decisions.clone());
        }
        if self.config.as_ref() != Some(&disk)
            && !(current == updated && self.config.as_ref() == Some(&comparable))
        {
            return Err(dm::error("Configuration changed in another client; reload before saving. Your draft is preserved"));
        }
        let mut expected = disk.clone();
        expected
            .automation
            .coordinator
            .get_or_insert_with(Default::default)
            .decisions = Some(updated.clone());
        if current != updated {
            if current.trust_mode == "secure_approval"
                && self.decision_settings_proof.trim().is_empty()
                && dm::broker_available(&current)
            {
                self.decision_center.submit(
                    paths.root.clone(),
                    self.client_context.client_id.clone(),
                    crate::decision_center::Action::Policy(updated),
                );
                self.decision_center.show();
                self.set_status(UiStatusLevel::Info,"Independent policy approval requested. Apply its ticket in Decisions, then press s again to finish saving; your draft is preserved.",None);
                return Ok(false);
            }
            let proof = if self.decision_settings_proof.trim().is_empty() {
                None
            } else {
                Some(
                    serde_json::from_str::<dm::Attestation>(&self.decision_settings_proof)
                        .map_err(|e| dm::error(e.to_string()))?,
                )
            };
            dm::configure_attested(&paths.root, updated, proof.as_ref())?;
        }
        let saved = macc_core::load_canonical_config(&paths.config_path)?;
        if saved != expected {
            return Err(dm::error("Configuration changed while saving the decision policy; reload before saving other settings. Your draft is preserved"));
        }
        self.config = Some(saved);
        self.decision_settings_proof.clear();
        Ok(true)
    }
}
fn policy_from(config: Option<&CanonicalConfig>) -> dm::Policy {
    config
        .and_then(|c| c.automation.coordinator.as_ref())
        .and_then(|c| c.decisions.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_navigation_include_all_decision_fields() {
        let (_tmp, mut state) = crate::workspace_tests::fixture();
        state.jump_config_tab(7);
        assert_eq!(AppState::CONFIG_TAB_NAMES[7], "Decisions");
        assert_eq!(AppState::config_tab_fields(7).len(), FIELDS.len());
        for (key, expected) in [
            ("similarity_hints", "false"),
            ("similarity_backend", "lexical"),
            ("investigation.max_rounds", "2"),
            ("governance_rules", "[]"),
        ] {
            let i = FIELDS.iter().position(|f| f.key == key).unwrap();
            assert_eq!(state.decision_field_value(i), expected);
        }
        for _ in 0..FIELDS.len() {
            state.navigate_config_next();
        }
        assert_eq!(state.current_config_field(), Some((3, FIELDS.len() - 1)));
    }
    #[test]
    fn native_save_preserves_other_drafts_and_unknown_policy_fields_until_explicit_save() {
        let (tmp, mut state) = crate::workspace_tests::fixture();
        // Disabled policy can be configured without enrolling a Git authority.
        let mut p = dm::Policy::default();
        p.approval_manifests
            .insert("APP".into(), vec!["contract.md".into()]);
        state.config.as_mut().unwrap().automation.coordinator = Some(Default::default());
        state
            .config
            .as_mut()
            .unwrap()
            .automation
            .coordinator
            .as_mut()
            .unwrap()
            .decisions = Some(p.clone());
        state.working_copy = state.config.clone();
        macc_core::save_canonical_config(
            &ProjectPaths::from_root(tmp.path()),
            state.config.as_ref().unwrap(),
        )
        .unwrap();
        let before = std::fs::read(tmp.path().join(".macc/macc.yaml")).unwrap();
        assert!(state.set_decision_field(1, "critical"));
        assert_eq!(
            state.decision_policy().approval_manifests,
            p.approval_manifests
        );
        assert_eq!(
            before,
            std::fs::read(tmp.path().join(".macc/macc.yaml")).unwrap()
        );
        let disk = macc_core::load_canonical_config(&tmp.path().join(".macc/macc.yaml")).unwrap();
        assert_eq!(
            policy_from(Some(&disk)).human_question_min_risk,
            dm::Consultation::High
        );
        state.working_copy.as_mut().unwrap().settings.debug = true;
        assert!(state.persist_decision_settings().unwrap());
        let disk = macc_core::load_canonical_config(&tmp.path().join(".macc/macc.yaml")).unwrap();
        assert_eq!(
            policy_from(Some(&disk)).human_question_min_risk,
            dm::Consultation::Critical
        );
        assert!(!disk.settings.debug);
        assert!(state.working_copy.as_ref().unwrap().settings.debug);
        assert_eq!(
            policy_from(Some(&disk)).approval_manifests,
            p.approval_manifests
        );
    }
    #[test]
    fn enabling_without_native_authority_cannot_bypass_activation_checks() {
        let (tmp, mut state) = crate::workspace_tests::fixture();
        let file = tmp.path().join(".macc/macc.yaml");
        let before = std::fs::read(&file).unwrap();
        assert!(state.set_decision_field(0, "true"));
        assert!(state.persist_decision_settings().is_err());
        assert_eq!(std::fs::read(&file).unwrap(), before);
        assert!(state.decision_policy().enabled);
    }
    #[test]
    fn invalid_policy_or_concurrent_config_does_not_write_or_discard_draft() {
        let (tmp, mut state) = crate::workspace_tests::fixture();
        let file = tmp.path().join(".macc/macc.yaml");
        let before = std::fs::read(&file).unwrap();
        assert!(state.set_decision_field(2, "1"));
        assert!(state.persist_decision_settings().is_err());
        assert_eq!(std::fs::read(&file).unwrap(), before);
        assert_eq!(state.decision_policy().max_context_bytes, 1);
        assert!(state.set_decision_field(2, "32768"));
        assert!(state.set_decision_field(1, "critical"));
        std::fs::write(&file, "tools:\n  enabled: []\nsettings:\n  debug: true\n").unwrap();
        let changed = std::fs::read(&file).unwrap();
        assert!(state.persist_decision_settings().is_err());
        assert_eq!(std::fs::read(&file).unwrap(), changed);
        assert_eq!(
            state.decision_policy().human_question_min_risk,
            dm::Consultation::Critical
        );
    }
}

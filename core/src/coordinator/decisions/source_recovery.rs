//! Supervisor source recovery is attributed machine evidence, never a human answer.
use super::{store::*, *};
use serde_json::json;

pub(super) fn validate_source_path(path: &str) -> crate::Result<()> {
    if path.is_empty()
        || path.chars().any(char::is_control)
        || path.starts_with('/')
        || path.contains('\\')
        || path.split('/').any(|s| {
            s.is_empty()
                || s == "."
                || s == ".."
                || s == ".git"
                || s == ".macc"
                || s == ".env"
                || s.starts_with(".env.")
        })
        || path.contains(['*', '?'])
        || path.ends_with(".prd.json")
        || path == "prd.json"
        || matches!(path, "AGENTS.md" | "GEMINI.md" | "CLAUDE.md")
        || path.starts_with(".codex/")
        || path.starts_with(".agents/")
        || path.starts_with(".claude/")
        || path.starts_with(".gemini/")
    {
        return Err(error(format!(
            "Source repair path is not an application source path: {path}"
        )));
    }
    Ok(())
}
impl Store {
    /// No CLI command exposes this authority to a performer. Called by the
    /// stopped supervisor only after independent verification and publication.
    pub fn complete_source_repairs(
        &mut self,
        refs: &[DecisionRef],
        base: &str,
        commit: &str,
        proof: &str,
    ) -> crate::Result<Vec<String>> {
        let _controller = controller_lock(&self.root)?;
        let ownership = crate::process_ownership::OwnershipStore::load(&self.root)?;
        if ownership.records.iter().any(|record| {
            record.process.kind == crate::process_ownership::ProcessKind::Coordinator
                && record
                    .process
                    .pid
                    .is_some_and(|pid| crate::coordinator::helpers::is_pid_running(pid as i64))
        }) {
            return Err(error(
                "Stop the coordinator before reconciling a supervised source repair",
            ));
        }
        let _lock = self.mutation_lock()?;
        let p = policy(&self.root)?;
        if !p.enabled
            || p.human_question_min_risk != Consultation::NeverAsk
            || proof.trim().is_empty()
        {
            return Err(error(
                "Source recovery requires explicit never_ask delegation and verified evidence",
            ));
        }
        if ![base, commit]
            .iter()
            .all(|id| id.len() == 40 && id.bytes().all(|c| c.is_ascii_hexdigit()))
            || base == commit
        {
            return Err(error("Source recovery requires distinct exact Git commits"));
        }
        let ancestry = crate::git::run_git_output_mapped(
            &self.root,
            &["merge-base", "--is-ancestor", base, commit],
            "verify repair ancestry",
        )?;
        if !ancestry.status.success() {
            return Err(error("Repair is not descended from its reviewed base"));
        }
        let mut seen = std::collections::BTreeSet::new();
        let output = crate::git::run_git_output_mapped(
            &self.root,
            &["diff", "--name-only", base, commit],
            "inspect published source repair",
        )?;
        if !output.status.success() {
            return Err(error("Cannot verify published repair diff"));
        }
        let changed =
            String::from_utf8(output.stdout).map_err(|_| error("Non UTF-8 repair paths"))?;
        let config =
            crate::load_canonical_config(&crate::ProjectPaths::from_root(&self.root).config_path)?;
        let supervisor = config
            .automation
            .supervisor
            .as_ref()
            .cloned()
            .unwrap_or_default();
        if !supervisor.repair_enabled || !supervisor.ai_verification_enabled {
            return Err(error("Source recovery is not authorized without enabled repair and independent verification"));
        }
        let reference = crate::config::CoordinatorConfigResolved::resolve(
            config.automation.coordinator.as_ref(),
        )
        .reference_branch;
        let published = crate::git::run_git_output_mapped(
            &self.root,
            &["merge-base", "--is-ancestor", commit, &reference],
            "verify published supervisor repair",
        )?;
        if !published.status.success() {
            return Err(error(
                "Source repair is not published to the reference branch",
            ));
        }
        let mut decisions = vec![];
        for reference in refs {
            if !seen.insert(&reference.id) {
                return Err(error("Duplicate source recovery decision"));
            }
            let d = self.decision(&reference.id)?;
            if d.version != reference.version
                || d.status != "blocked_external"
                || d.request.kind != "intervention"
                || d.request.impact.production
                || d.request.impact.irreversible
                || d.request.impact.security_privacy
                || d.request.impact.product_scope
                || d.request.impact.shared_contract
                || d.request.operation_id.is_some()
                || d.request.operation_spec.is_some()
                || d.request.response_fields.iter().any(|field| field.required)
                || d.request
                    .options
                    .iter()
                    .any(|option| option.intervention_response.is_some())
                || d.request.has_effects()
            {
                return Err(error(
                    "Repair decision changed or requires separate authority",
                ));
            }
            let plan = self.plan(&d.plan_id)?;
            if d.requires_human_renewal
                || d.subject_digest
                    != digest(&super::graph::contract(super::graph::task(
                        &plan.graph,
                        &d.request.task_id,
                    )?))
            {
                return Err(error("Source recovery task contract or authority changed"));
            }

            if super::graph::task(&plan.graph, &d.request.task_id)?
                .get("gate")
                .is_some()
            {
                return Err(error("Human gates cannot be recovered by source edits"));
            }
            let mut authority = d.request.clone();
            authority.kind = "question".into();
            let triage = self.triage(&plan, &mut authority, &p)?;
            if triage.mandatory || !triage.report.missing_sources.is_empty() {
                return Err(error(
                    "Current governance requires separate authority or evidence",
                ));
            }
            let paths = &d
                .request
                .intervention
                .as_ref()
                .ok_or_else(|| error("Missing intervention"))?
                .source_repair_paths;
            if paths.is_empty() {
                return Err(error(
                    "External actions cannot be satisfied by a source repair",
                ));
            }
            let mut protected = crate::collect_context_targets(&self.root, &config);
            protected.push(plan.source.clone());
            if let Some(coordinator) = &config.automation.coordinator {
                protected.extend(coordinator.prd_files.iter().cloned());
                protected.extend(coordinator.prd_file.iter().cloned());
                protected.extend(coordinator.task_registry_file.iter().cloned());
            }
            for path in paths {
                validate_source_path(path)?;
                crate::project_ownership::validate_path(
                    &crate::ProjectPaths::from_root(&self.root),
                    path,
                )?;
                if protected
                    .iter()
                    .any(|target| target == path || target.starts_with(&format!("{path}/")))
                {
                    return Err(error(format!("Source recovery scope includes a configured context or task contract: {path}")));
                }
                if !changed
                    .lines()
                    .any(|c| c == path || c.starts_with(&format!("{path}/")))
                {
                    return Err(error(format!("No published repair evidence for {path}")));
                }
            }
            decisions.push(d);
        }
        let tx = sql(self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        let mut ids = vec![];
        for mut d in decisions {
            d.version += 1;
            d.status = "resolved".into();
            d.resolution = Some(Resolution {
                text: format!("Verified source repair {commit}: {proof}"),
                selected_option_id: None,
                actor: "supervisor".into(),
                origin: "supervisor_source_repair".into(),
                policy_digest: digest(&p),
                recorded_at: now(),
                advisory: false,
                source_revision: Some(commit.into()),
                fields: Default::default(),
                outcome: "source_repaired".into(),
                conditions: vec![],
                artifacts: vec![],
                answered_version: Some(d.version - 1),
                applied_patch: None,
                effects: "none".into(),
                patch_applied: false,
                attestation: None,
            });
            sql(tx.execute(
                "UPDATE requests SET payload=?2 WHERE id=?1",
                rusqlite::params![d.id, encode(&d)?],
            ))?;
            let mut plan: Plan = decode(&sql(tx.query_row(
                "SELECT payload FROM plans WHERE id=?1",
                [&d.plan_id],
                |r| r.get::<_, String>(0),
            ))?)?;
            plan.decision_epoch += 1;
            save_plan(&tx, &plan)?;
            event(
                &tx,
                &plan.id,
                "decision.source_repaired",
                &json!({"decision":d,"base":base,"commit":commit,"proof":proof,"origin":"supervisor_source_repair"}),
            )?;
            ids.push(d.id);
        }
        sql(tx.commit())?;
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn git(root: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().into()
    }
    fn request(key: &str, paths: Vec<String>) -> Request {
        serde_json::from_value(json!({"idempotency_key":key,"task_id":"A","kind":"intervention","question":"Implement the missing existing API function","context":"The source function required by the contract is missing","investigated":["Read API source and existing contract"],"recommendation":"Repair the declared application source and independently validate it","risk":"low","impact":{"evidence":["Missing function in src/api.rs"]},"intervention":{"source_repair_paths":paths,"action":"Implement the missing source function","inaccessible_reason":"Outside this performer's writable scope","expected_result":"Existing contract implemented and independently verified","verification":"Tests plus independent verification"}})).unwrap()
    }
    #[test]
    fn source_recovery_requires_published_exact_scoped_commit_and_never_claims_a_human_action() {
        let (temp, mut store, plan, _, mut p) = super::super::tests::fixture();
        let root = temp.path();
        p.human_question_min_risk = Consultation::NeverAsk;
        let mut config =
            crate::load_canonical_config(&crate::ProjectPaths::from_root(root).config_path)
                .unwrap();
        config.automation.coordinator.as_mut().unwrap().decisions = Some(p.clone());
        config.automation.supervisor = Some(crate::supervisor::SupervisorConfig {
            repair_enabled: true,
            ai_verification_enabled: true,
            ..Default::default()
        });
        std::fs::write(root.join(".macc/macc.yaml"), config.to_yaml().unwrap()).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/api.rs"), "// missing implementation\n").unwrap();
        git(root, &["add", "src/api.rs"]);
        git(root, &["commit", "-m", "base"]);
        let base = git(root, &["rev-parse", "HEAD"]);
        let d = store
            .submit(
                &plan,
                request("repair", vec!["src/api.rs".into()]),
                "agent:A",
                "agent",
                &p,
            )
            .unwrap();
        assert_eq!(d.status, "blocked_external");
        let refs = vec![DecisionRef {
            id: d.id.clone(),
            version: d.version,
        }];
        assert!(store
            .complete_source_repairs(&refs, &base, &base, "verification")
            .is_err());
        std::fs::write(root.join("unrelated.txt"), "unrelated").unwrap();
        git(root, &["add", "unrelated.txt"]);
        git(root, &["commit", "-m", "unrelated"]);
        let unrelated = git(root, &["rev-parse", "HEAD"]);
        assert!(store
            .complete_source_repairs(&refs, &base, &unrelated, "verification")
            .is_err());
        assert_eq!(store.decision(&d.id).unwrap().status, "blocked_external");
        std::fs::write(root.join("src/api.rs"), "pub fn existing_contract() {}\n").unwrap();
        git(root, &["add", "src/api.rs"]);
        git(root, &["commit", "-m", "repair"]);
        let commit = git(root, &["rev-parse", "HEAD"]);
        let physical = store
            .submit(&plan, request("physical", vec![]), "agent:A", "agent", &p)
            .unwrap();
        assert!(store
            .complete_source_repairs(
                &[DecisionRef {
                    id: physical.id.clone(),
                    version: physical.version
                }],
                &base,
                &commit,
                "verification"
            )
            .is_err());
        assert!(store
            .complete_source_repairs(
                &[DecisionRef {
                    id: d.id.clone(),
                    version: d.version + 1
                }],
                &base,
                &commit,
                "verification"
            )
            .is_err());
        assert!(store
            .complete_source_repairs(&refs, &base, &commit, "")
            .is_err());
        assert_eq!(
            store
                .complete_source_repairs(
                    &refs,
                    &base,
                    &commit,
                    "Independent verification passed for the exact published source"
                )
                .unwrap(),
            vec![d.id.clone()]
        );
        let repaired = store.decision(&d.id).unwrap();
        assert_eq!(repaired.version, d.version + 1);
        assert_eq!(repaired.status, "resolved");
        let resolution = repaired.resolution.unwrap();
        assert_eq!(resolution.origin, "supervisor_source_repair");
        assert!(resolution.fields.is_empty());
        assert_eq!(resolution.outcome, "source_repaired");
        assert_eq!(
            store.decision(&physical.id).unwrap().status,
            "blocked_external"
        );
        store.verify().unwrap();
    }
    #[test]
    fn source_scope_refuses_state_secrets_context_and_contract_files() {
        for path in [
            ".macc/state/x",
            ".git/config",
            ".env",
            "apps/.env.local",
            "../src/x",
            "/tmp/x",
            "AGENTS.md",
            "docs/p.prd.json",
            ".codex/config.toml",
            "src/invalid\nname.rs",
        ] {
            assert!(validate_source_path(path).is_err(), "{path}");
        }
        assert!(validate_source_path("apps/api/src/routes.rs").is_ok());
    }
}

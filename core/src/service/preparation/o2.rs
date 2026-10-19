use super::*;
pub(super) fn prepare(paths: &ProjectPaths, plan: &Plan, report: &mut Report) -> Result<()> {
    let config = crate::load_canonical_config(&paths.config_path)?;
    if !crate::o2::activation::requested(&config) {
        return Ok(());
    }
    crate::o2::activation::require_ready(&paths.root)?;
    crate::o2::memory::fts_rebuild(&paths.root)?;
    let policy = crate::o2::policy::load(&paths.root)?;
    if policy.memory.enabled && policy.memory.mcp_enabled {
        for tool in &plan.tools {
            tool.spec
                .performer
                .as_ref()
                .and_then(|p| p.scoped_mcp.as_ref())
                .ok_or_else(|| {
                    MaccError::Validation(format!(
                        "{} has no scoped MCP registration adapter",
                        tool.spec.id
                    ))
                })?
                .verify(&paths.root)?;
        }
        report.completed.push(
            "Scoped MCP registrations verified; credentials are minted for each leased runner"
                .into(),
        );
    }
    if policy.adaptive_compaction.enabled && policy.adaptive_compaction.configure_native_auto {
        for tool in &plan.tools {
            let Some(profile) = tool
                .spec
                .performer
                .as_ref()
                .and_then(|p| p.compaction.as_ref())
            else {
                report.completed.push(format!(
                    "{}: checkpoint rotation ready; native auto configuration unsupported",
                    tool.spec.id
                ));
                continue;
            };
            let Some(native) = profile.native_auto.as_ref() else {
                continue;
            };
            if native.unit == "tokens" && policy.adaptive_compaction.native_token_threshold == 0 {
                return Err(MaccError::Validation("Set the native token threshold in Apply configuration before enabling native auto compaction".into()));
            }
            let probe = crate::o2::compaction::probe(&paths.root, &tool.spec.id)?;
            if probe.state != "version_help_observed" {
                return Err(MaccError::Validation(format!(
                    "{} native capability verification failed: {}",
                    tool.spec.id,
                    probe.diagnostics.join("; ")
                )));
            }
            crate::o2::compaction::prepare_configuration(&paths.root, &tool.spec.id)?;
        }
        report.completed.push("Native compaction capabilities verified; per-worktree settings are applied under the runner lease".into());
    }
    if policy.memory.semantic_enabled {
        if let Some(approval) = &plan.inputs.semantic_approval {
            crate::o2::semantic::apply_gate(
                &paths.root,
                &approval.report_id,
                &approval.checksum,
                true,
                true,
            )?;
        }
        let status = crate::o2::semantic::status(&paths.root, &Default::default())?;
        if status["approved_event"].is_null() {
            let (kind, message, data) = if let Some(suite) = &plan.inputs.semantic_suite {
                let suite = serde_json::from_value(suite.clone())
                    .map_err(|e| MaccError::Validation(format!("Invalid labelled suite: {e}")))?;
                let result = crate::o2::semantic::benchmark(&paths.root, suite)?;
                (
                    "semantic_review",
                    "Review the benchmark results below. Only a passing benchmark can be approved.",
                    result,
                )
            } else {
                ("semantic_suite","Provide the labelled benchmark suite JSON (4–100 cases referencing reviewed memory IDs), or adjust the semantic option in Apply configuration.",serde_json::json!({"access":{},"cases":[{"query":"representative question","relevant_ids":["reviewed-memory-id"]}],"top_k":5}))
            };
            report.question = Some(Question {
                kind: kind.into(),
                message: message.into(),
                data,
            });
            return Err(MaccError::Validation(message.into()));
        }
        report
            .completed
            .push("Current semantic quality approval verified".into());
    }
    report
        .completed
        .push("O2 schema and full-text index verified".into());
    Ok(())
}

pub(super) fn verify(paths: &ProjectPaths, plan: &Plan) -> Result<()> {
    let p = crate::o2::policy::load(&paths.root)?;
    if p.memory.enabled && p.memory.mcp_enabled {
        for tool in &plan.tools {
            tool.spec
                .performer
                .as_ref()
                .and_then(|p| p.scoped_mcp.as_ref())
                .ok_or_else(|| {
                    MaccError::Validation(format!("{} has no scoped MCP adapter", tool.spec.id))
                })?
                .verify(&paths.root)?;
        }
    }
    if p.adaptive_compaction.enabled && p.adaptive_compaction.configure_native_auto {
        let probes = crate::o2::compaction::inspect(&paths.root)?;
        for tool in &plan.tools {
            if tool
                .spec
                .performer
                .as_ref()
                .and_then(|p| p.compaction.as_ref())
                .is_some_and(|p| p.native_auto.is_some())
                && !probes
                    .iter()
                    .any(|p| p.tool_id == tool.spec.id && p.state == "version_help_observed")
            {
                return Err(MaccError::Validation(format!(
                    "{} native capabilities changed; complete Apply again",
                    tool.spec.id
                )));
            }
        }
    }
    if p.memory.semantic_enabled
        && crate::o2::semantic::status(&paths.root, &Default::default())?["approved_event"]
            .is_null()
    {
        return Err(MaccError::Validation(
            "Semantic quality evidence is not current; complete the Apply assistant".into(),
        ));
    }
    Ok(())
}

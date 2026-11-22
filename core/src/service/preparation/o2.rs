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
            phase(
                &paths.root,
                &format!("Verifying {} MCP connection", tool.spec.id),
            )?;
            crate::o2::mcp::verification::verify(&paths.root, &tool.spec)?;
        }
        report.completed.push(
            "Scoped MCP configuration, startup, handshake, catalog and authorized control read verified; control leases revoked"
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
        if let Some(question) = semantic_question(&paths.root, &plan.inputs)? {
            let error = MaccError::Validation(question.message.clone());
            report.question = Some(question);
            return Err(error);
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

fn semantic_question(root: &Path, inputs: &Inputs) -> Result<Option<Question>> {
    use crate::o2::semantic;
    let access = Default::default();
    let status = semantic::inspect(root, &access)?;
    let available = status.documents;
    let minimum = status.minimum_benchmark_documents;
    if available < minimum {
        return Ok(Some(Question {
            kind: "semantic_insufficient_memories".into(),
            message: format!("Eligible reviewed memories: {available}. The semantic benchmark requires at least {minimum}. Disable semantic search for this project to continue Apply, or retry after reviewing more memories."),
            data: serde_json::json!({"available": available, "minimum": minimum, "path": "memory.semantic_enabled", "value": "false"}),
        }));
    }
    let approved = if let Some(approval) = &inputs.semantic_approval {
        semantic::apply_gate(root, &approval.report_id, &approval.checksum, true, true)?;
        semantic::inspect(root, &access)?.approved_event
    } else {
        status.approved_event
    };
    if approved.is_some() {
        return Ok(None);
    }
    if let Some(suite) = &inputs.semantic_suite {
        let suite = serde_json::from_value(suite.clone())
            .map_err(|e| MaccError::Validation(format!("Invalid labelled suite: {e}")))?;
        return Ok(Some(Question {
            kind: "semantic_review".into(),
            message:
                "Review the benchmark results below. Only a passing benchmark can be approved."
                    .into(),
            data: semantic::benchmark(root, suite)?,
        }));
    }
    Ok(Some(Question {
        kind: "semantic_suite".into(),
        message: format!("Eligible reviewed memories: {available}. Provide a labelled JSON suite with 4–100 queries referencing reviewed memory IDs."),
        data: serde_json::json!({"access":{},"cases":[{"query":"representative question","relevant_ids":["reviewed-memory-id"]}],"top_k":5}),
    }))
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
            let registration = tool
                .spec
                .performer
                .as_ref()
                .and_then(|p| p.scoped_mcp.as_ref())
                .unwrap();
            if !crate::o2::mcp::verification::current(&paths.root, &tool.spec.id, registration)? {
                return Err(MaccError::Validation(format!(
                    "{} MCP connection verification missing or stale; complete Apply again",
                    tool.spec.id
                )));
            }
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
        && crate::o2::semantic::inspect(&paths.root, &Default::default())?
            .approved_event
            .is_none()
    {
        return Err(MaccError::Validation(
            "Semantic quality evidence is not current; complete the Apply assistant".into(),
        ));
    }
    Ok(())
}

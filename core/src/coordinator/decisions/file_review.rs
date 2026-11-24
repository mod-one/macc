//! Operator review of edits already present in a registered PRD projection.
use super::*;
use serde_json::Value;

pub(super) fn drift_error(plan: &Plan, actual: &Value) -> crate::MaccError {
    let diff = graph_diff(&plan.graph, actual);
    let mut details = Vec::new();
    if let Some(rows) = diff["tasks"].as_array() {
        for row in rows.iter().take(12) {
            details.push(format!(
                "{} {} ({})",
                row["task"].as_str().unwrap_or("?"),
                row["change"].as_str().unwrap_or("?"),
                row["fields"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if rows.len() > 12 {
            details.push(format!("{} more tasks", rows.len() - 12));
        }
    }
    if diff["plan_fields"]
        .as_array()
        .is_some_and(|a| !a.is_empty())
    {
        details.push(format!("PRD fields {}", diff["plan_fields"]));
    }
    if details.is_empty() {
        details.push("Task order or structure changed".into());
    }
    crate::MaccError::PrdPlanDrift {
        plan_id: plan.id.clone(),
        message: format!("PRD edits awaiting approval\n  File: {}\n  Registered plan: version {} on {}\n\nChanges since approval\n  - {}\n\nThe coordinator uses the approved task definitions. Confirm the file edits to sync them.\n'prds reset' only resets queue progress.\n\nReview command: macc decisions graph reconcile-file --plan {}",
            plan.source, plan.graph_version, plan.reference_branch, details.join("\n  - "), plan.id),
    }
}

pub struct FileReview {
    pub patch: Patch,
    pub source_digest: String,
    pub challenge: Value,
}

impl Store {
    /// Return a proposal only when the governed patch reproduces the entire disk graph.
    pub fn review_prd_file(&self, plan: &Plan) -> crate::Result<Option<FileReview>> {
        let config =
            crate::load_canonical_config(&crate::ProjectPaths::from_root(&self.root).config_path)?;
        if plan.reference_branch
            != crate::config::CoordinatorConfigResolved::resolve(
                config.automation.coordinator.as_ref(),
            )
            .reference_branch
        {
            return Err(error(
                "The selected plan belongs to a different reference branch",
            ));
        }
        let disk = self.projection_diff(&self.root, plan)?;
        let actual = &disk["actual"];
        if actual == &plan.graph {
            return Ok(None);
        }
        let path = super::graph::projection_path(&self.root, &plan.source)?;
        let validation =
            crate::prd_generation::validation::validate_prd_content(&actual.to_string(), &path);
        if !validation.ok {
            return Err(error(format!(
                "Correct {} before reviewing: {}",
                plan.source,
                validation.errors.join("; ")
            )));
        }
        let patch = self.edit_patch(plan, actual, plan.graph_version, "disk-review")?;
        if patch.operations.is_empty() && patch.metadata_changes.is_empty() {
            return Err(error("Only task order, execution fields or empty values changed. Restore their previous values from the durable plan before syncing."));
        }
        let proposed = self.preview_patch(plan, &patch, &registry(&self.root)?)?;
        if proposed != *actual {
            return Err(error("The patch cannot reproduce the edited PRD exactly. Preserve task order and execution history, and use an explicit graph patch to retire tasks."));
        }
        let source_digest = digest(actual);
        let challenge = graph_challenge(plan, &patch, Some(&source_digest), &policy(&self.root)?);
        Ok(Some(FileReview {
            patch,
            source_digest,
            challenge,
        }))
    }
}

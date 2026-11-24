use super::{dm, json, operator, read, Path, Result, Value};

pub(super) fn reconcile_file(
    root: &Path,
    id: &str,
    confirm: Option<&str>,
    proof: Option<&Path>,
) -> Result<Value> {
    let store = dm::Store::inspect_registered(root)?
        .ok_or_else(|| dm::error("No registered decision plan in this project"))?;
    let plan = store.plan(id)?;
    let Some(review) = store.review_prd_file(&plan)? else {
        return Ok(
            json!({"source":plan.source, "status":"already in sync", "next":if plan.projected_version == plan.graph_version {"macc coordinator sync"} else {"macc decisions recover"}}),
        );
    };
    let token = dm::digest(&review.challenge);
    if let Some(confirmed) = confirm {
        if confirmed != token {
            return Err(dm::error("Preview expired or confirmation token incorrect. Review the edited PRD again before confirming."));
        }
        let actor = operator(root)?;
        let proof = proof.map(|path| read(path, 65536)).transpose()?;
        let _queue_lock = macc_core::fs_lock::AdvisoryLock::acquire(
            &root.join(".macc/state/prd-queue.lock"),
            std::time::Duration::ZERO,
            "PRD reconciliation",
        )?;
        drop(store);
        let mut store = dm::Store::open(root)?;
        let applied = store.reconcile_projection_attested(
            root,
            &review.patch,
            &review.source_digest,
            &actor,
            proof.as_ref(),
        )?;
        store.project_selected(root, Some(&applied.id)).map_err(|e| dm::error(format!(
            "Plan v{} was committed but publication is pending: {e}. Run macc decisions recover, then macc coordinator sync.", applied.graph_version)))?;
        return Ok(
            json!({"source":applied.source,"status":"reconciled","graph_version":applied.graph_version,"next":"macc coordinator sync"}),
        );
    }
    let signed = dm::policy(root)?.trust_mode == "secure_approval";
    let suffix = if signed {
        " --attestation-file signed-attestation.json"
    } else {
        ""
    };
    let mut report = json!({"source":plan.source,"graph_version":plan.graph_version,"changes":review.patch.describe(),
        "operations":review.patch.operations,"metadata_changes":review.patch.metadata_changes,"confirmation":token,
        "next":format!("macc decisions graph reconcile-file --plan {} --confirm {token}{suffix}",plan.id)});
    if signed {
        report["challenge"] = review.challenge;
    }
    Ok(report)
}

pub(crate) fn confirm_sync_edits(root: &Path, id: &str) -> Result<()> {
    use std::io::{self, Write};
    let preview = reconcile_file(root, id, None, None)?;
    let Some(token) = preview["confirmation"].as_str() else {
        return Ok(());
    };
    if preview.get("challenge").is_some() {
        return Err(dm::error(format!("A signed approval is required. Review the challenge with macc decisions graph reconcile-file --plan {id}")));
    }
    eprintln!("\nProposed values");
    for op in preview["operations"].as_array().into_iter().flatten() {
        if let (Some(id), Some(fields)) = (op["task_id"].as_str(), op["changes"].as_object()) {
            eprintln!("  {id}");
            for (field, value) in fields {
                print_field(field, value);
            }
        } else {
            eprintln!("  {op:#}");
        }
    }
    if let Some(fields) = preview["metadata_changes"].as_object() {
        for (field, value) in fields {
            print_field(field, value);
        }
    }
    eprint!("\nApprove these edits and continue sync? [y/N] ");
    io::stderr().flush().map_err(|e| dm::error(e.to_string()))?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|e| dm::error(e.to_string()))?;
    if !matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes" | "o" | "oui"
    ) {
        return Err(dm::error(
            "Sync cancelled. Your file edits are preserved and the plan was not updated.",
        ));
    }
    let result = reconcile_file(root, id, Some(token), None)?;
    eprintln!("Plan updated. Continuing sync for {}.", result["source"]);
    Ok(())
}

fn print_field(field: &str, value: &Value) {
    eprintln!("    {field}");
    if let Some(items) = value.as_array().filter(|items| !items.is_empty()) {
        for item in items {
            eprintln!(
                "      - {}",
                item.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| item.to_string())
            );
        }
    } else {
        eprintln!(
            "      {}",
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string())
        );
    }
}

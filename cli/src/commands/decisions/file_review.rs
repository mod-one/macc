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

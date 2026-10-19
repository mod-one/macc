//! MACC defects have a different repair destination from application source.
use super::{
    repair::Diagnosis,
    report::{slug, write_json, Intervention},
};
use macc_core::{MaccError, ProjectPaths, Result};
use serde_json::json;

pub(super) fn publish(
    paths: &ProjectPaths,
    report: &Intervention,
    diagnosis: &Diagnosis,
) -> Result<()> {
    if diagnosis.macc_findings.is_empty() && !diagnosis.macc_repair_required {
        return Ok(());
    }
    let value = json!({"schema_version":1,"incident_id":report.incident.id,"observed_at":chrono::Utc::now().to_rfc3339(),"application_root":paths.root,"evidence_directory":report.log_dir,"repair_destination":"MACC source repository and installed runtime","status":"reported_unfixed","blocks_application_recovery":diagnosis.macc_repair_required,"findings":diagnosis.macc_findings,"next_action":"Fix and validate MACC in its source repository, install the corrected build, restart the supervisor, then resume retained tasks. Application changes do not repair the installed MACC runtime."});
    write_json(
        &paths
            .root
            .join(".macc/reports/macc-defects")
            .join(format!("{}.json", slug(&report.incident.id))),
        &value,
    )
}
pub(super) fn ensure_application_repair(diagnosis: &Diagnosis) -> Result<()> {
    if diagnosis.macc_repair_required {
        return Err(MaccError::Validation("MACC product update required; application work and diagnosis retained. See .macc/reports/macc-defects/. Fix/install MACC and restart the supervisor before resuming these tasks.".into()));
    }
    if !diagnosis.external_prerequisites.is_empty() {
        return Err(MaccError::Validation(format!(
            "External verification or authority required; source repair cannot satisfy: {}",
            diagnosis.external_prerequisites.join("; ")
        )));
    }
    Ok(())
}
/// Reinstalling an uncommitted build must invalidate a product diagnosis too.
pub(super) fn runtime_identity() -> serde_json::Value {
    let metadata = std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::metadata(p).ok());
    json!({"build":env!("MACC_BUILD_SHA"),"bytes":metadata.as_ref().map(|m|m.len()),
        "modified_ns":metadata.and_then(|m|m.modified().ok()).and_then(|t|t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d|d.as_nanos().to_string())})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn product_blockers_and_external_authority_do_not_enter_application_repair() {
        let mut d:Diagnosis=serde_json::from_value(json!({"summary":"mixed incident","repairable":true,"macc_repair_required":true,"macc_findings":["runner dropped acknowledgement"]})).unwrap();
        assert!(ensure_application_repair(&d)
            .unwrap_err()
            .to_string()
            .contains("MACC product update required"));
        d.macc_repair_required = false;
        assert!(ensure_application_repair(&d).is_ok()); // Improvement hints alone do not block project repair.
        d.external_prerequisites
            .push("macOS device verification".into());
        assert!(ensure_application_repair(&d)
            .unwrap_err()
            .to_string()
            .contains("External verification"));
    }
}

#[cfg(test)]
mod report_tests {
    use super::*;
    #[test]
    fn required_product_fix_is_archived_with_original_evidence_and_unfixed_status() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(temp.path());
        let d:Diagnosis=serde_json::from_value(json!({"summary":"runner fault","repairable":true,"macc_repair_required":true,"macc_findings":["protocol output omitted"]})).unwrap();
        let report:Intervention=serde_json::from_value(json!({"incident":{"id":"incident-1","run_id":"run-1","epoch":1,"reason":"failure","run_status":"blocked","tasks":[],"recent_events":[]},"attempt":1,"status":"diagnosing","tool":"future-tool","diagnosis":d,"worktree":null,"validation":[],"commit":null,"restart_pid":null,"detail":"","log_dir":"evidence/incident-1"})).unwrap();
        publish(&paths, &report, &d).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                temp.path()
                    .join(".macc/reports/macc-defects/incident-1.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(saved["status"], "reported_unfixed");
        assert_eq!(saved["blocks_application_recovery"], true);
        assert_eq!(saved["evidence_directory"], "evidence/incident-1");
        assert!(ensure_application_repair(&d).is_err());
    }
}

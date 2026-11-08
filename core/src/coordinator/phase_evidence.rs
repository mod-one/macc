//! Durable raw and interpreted phase output, including early validation failures.
use crate::{MaccError, Result};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Output,
};
pub struct PhaseEvidence {
    root: PathBuf,
    task: String,
    phase: String,
    path: PathBuf,
    record: Value,
}
impl PhaseEvidence {
    pub fn capture(
        root: &Path,
        task: &str,
        phase: &str,
        attempt: &str,
        source: &str,
        out: &Output,
        expected: Option<&str>,
    ) -> Result<Self> {
        let safe = |s: &str| {
            let name = s
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            if name.trim_matches('.').is_empty() {
                "unknown".into()
            } else {
                name
            }
        };
        let path = root
            .join(".macc/log/performer")
            .join(safe(task))
            .join("phases")
            .join(format!("{}-{}", safe(phase), safe(source)));
        std::fs::create_dir_all(&path).map_err(|source| io(&path, source))?;
        let raw = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let projected = super::result_protocol::projection(&raw);
        let mut artifacts = serde_json::Map::new();
        for (name, bytes) in [
            ("stdout.log", out.stdout.as_slice()),
            ("stderr.log", out.stderr.as_slice()),
            ("projected.txt", projected.as_bytes()),
        ] {
            let file = path.join(name);
            std::fs::write(&file, bytes).map_err(|e| io(&file, e))?;
            artifacts.insert(name.into(),json!({"bytes":bytes.len(),"digest":crate::coordinator::decisions::evidence_digest(bytes)}));
        }
        let eligible_acks: Vec<_> = projected
            .lines()
            .filter_map(|l| l.strip_prefix("MACC_DECISION_ACK:"))
            .map(str::trim)
            .collect();
        let record = json!({"schema_version":1,"task":task,"phase":phase,"attempt":attempt,"source":source,"exit_code":out.status.code(),"process_success":out.status.success(),"expected_context_digest":expected,"raw_ack_count":raw.lines().filter(|l|l.starts_with("MACC_DECISION_ACK:")).count(),"eligible_ack_digests":eligible_acks,"artifacts":artifacts,"status":"captured","validation_error":null,"recorded_at":chrono::Utc::now().to_rfc3339()});
        let evidence = Self {
            root: root.into(),
            task: task.into(),
            phase: phase.into(),
            path,
            record,
        };
        evidence.write()?;
        Ok(evidence)
    }
    pub fn settle(&mut self, status: &str, error: Option<&str>) -> Result<()> {
        self.record["status"] = json!(status);
        self.record["validation_error"] = json!(error);
        self.write()
    }
    fn write(&self) -> Result<()> {
        let file = self.path.join("outcome.json");
        let bytes = serde_json::to_vec_pretty(&self.record)
            .map_err(|e| MaccError::Validation(e.to_string()))?;
        crate::atomic_write(&crate::ProjectPaths::from_root(&self.root), &file, &bytes)?;
        Ok(())
    }
}
impl Drop for PhaseEvidence {
    fn drop(&mut self) {
        if let Err(error) = super::logs::aggregate_performer_logs(&self.root) {
            tracing::error!(%error,task=%self.task,phase=%self.phase,"Phase log aggregation failed; raw evidence retained");
        }
        let message=json!({"evidence":self.path,"source":self.record["source"],"attempt":self.record["attempt"],"exit_code":self.record["exit_code"],"validation_error":self.record["validation_error"]}).to_string();
        if let Err(error) = super::helpers::append_coordinator_event_with_severity(
            &self.root,
            "phase_output_recorded",
            &self.task,
            &self.phase,
            self.record["status"].as_str().unwrap_or("captured"),
            &message,
            "info",
        ) {
            tracing::error!(%error,"Phase evidence event failed; durable files retained");
        }
    }
}
fn io(path: &Path, source: std::io::Error) -> MaccError {
    MaccError::Io {
        path: path.display().to_string(),
        action: "persist phase evidence".into(),
        source,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn failed_validation_retains_raw_output_projection_and_aggregated_review() {
        use std::os::unix::process::ExitStatusExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let log = root.join(".macc/worktree/worker-01/.macc/log/performer/A.md");
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        std::fs::write(&log, "## Phase: review\nACK evidence\n").unwrap();
        let out = Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: b"assistant\nMACC_DECISION_ACK: exact\nREVIEW_VERDICT: CHANGES_REQUESTED\n"
                .to_vec(),
            stderr: Vec::new(),
        };
        let mut evidence = PhaseEvidence::capture(
            root,
            "A",
            "review",
            "attempt",
            "source-1",
            &out,
            Some("exact"),
        )
        .unwrap();
        evidence
            .settle("validation_failed", Some("independent check missing"))
            .unwrap();
        let path = evidence.path.clone();
        drop(evidence);
        let value: Value =
            serde_json::from_slice(&std::fs::read(path.join("outcome.json")).unwrap()).unwrap();
        assert_eq!(value["status"], "validation_failed");
        assert_eq!(value["eligible_ack_digests"], json!(["exact"]));
        assert_eq!(value["exit_code"], 0);
        assert_eq!(std::fs::read(path.join("stdout.log")).unwrap(), out.stdout);
        assert!(root.join(".macc/log/performer/worker-01--A.md").exists());
    }
}

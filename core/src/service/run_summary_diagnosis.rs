//! Durable operator-facing diagnosis. Native coordinator state owns recovery actions.
use super::*;
use crate::{MaccError, ProjectPaths};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorAnnotation {
    pub status: String,
    pub diagnosis: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_ids: Vec<String>,
    pub next_action: Option<String>,
}

impl SupervisorAnnotation {
    pub fn heading(&self) -> String {
        let tasks = self.task_ids.join(", ");
        if tasks.is_empty() {
            format!("Supervisor · {}", self.status)
        } else {
            format!("Supervisor · {} · {tasks}", self.status)
        }
    }
}

// Store the revisions from detection, before a repair can change the registry.
// Hashing the run ID also supports opaque IDs on Windows filesystems.
#[derive(Serialize, Deserialize)]
struct IncidentFile {
    run_id: String,
    epoch: i64,
    attempt: u64,
    reason: String,
    #[serde(default)]
    run_status: Option<String>,
    task_revisions: BTreeMap<String, String>,
    supervisor: SupervisorAnnotation,
}

fn path(paths: &ProjectPaths, run: &str, epoch: i64) -> PathBuf {
    let id = Sha256::digest(run.as_bytes());
    paths
        .root
        .join(".macc/log/coordinator/incidents")
        .join(format!("{id:x}-{epoch}.json"))
}

impl IncidentFile {
    fn from_report(report: &Value) -> Option<Self> {
        let intervention = report.get("intervention")?;
        let incident = intervention.get("incident")?;
        let mut task_revisions = BTreeMap::new();
        for raw in incident["tasks"].as_array()? {
            let task: Task = serde_json::from_value(raw.clone()).ok()?;
            let revision = crate::coordinator::control::task_revision(&task).ok()?;
            task_revisions.insert(task.id, revision);
        }
        let detail = intervention["detail"]
            .as_str()
            .filter(|s| !s.trim().is_empty());
        let summary = intervention
            .pointer("/diagnosis/summary")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty());
        let diagnosis = summary
            .or(detail)
            .or_else(|| intervention["activity"].as_str())
            .unwrap_or("Awaiting supervisor diagnosis.");
        let mut task_ids: Vec<String> = intervention
            .pointer("/diagnosis/task_ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        if task_ids.is_empty() {
            task_ids.extend(task_revisions.keys().cloned());
        }
        Some(Self {
            run_id: incident["run_id"].as_str()?.into(),
            epoch: incident["epoch"].as_i64()?,
            reason: incident["reason"].as_str().unwrap_or_default().into(),
            run_status: incident["run_status"].as_str().map(str::to_owned),
            attempt: intervention["attempt"].as_u64().unwrap_or_default(),
            task_revisions,
            supervisor: SupervisorAnnotation {
                status: intervention["status"].as_str()?.into(),
                diagnosis: diagnosis.into(),
                outcome: detail.filter(|d| *d != diagnosis).map(str::to_owned),
                task_ids,
                next_action: intervention["next_action"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            },
        })
    }

    fn annotation(&self, summary: &CoordinatorRunSummary) -> Option<SupervisorAnnotation> {
        if self.run_id != summary.run_id || self.epoch != summary.epoch {
            return None;
        }
        if let Some(id) = &summary.task_id {
            let revision = self.task_revisions.get(id)?;
            if summary.task_revision.as_ref() != Some(revision)
                || !self.supervisor.task_ids.contains(id)
            {
                return None;
            }
        }
        Some(self.supervisor.clone())
    }
}

/// Called by the supervisor's serialized report publisher on every phase and final outcome.
pub fn publish_incident(paths: &ProjectPaths, report: &Value) -> Result<()> {
    let file = IncidentFile::from_report(report)
        .ok_or_else(|| MaccError::Validation("Invalid supervisor incident report".into()))?;
    let destination = path(paths, &file.run_id, file.epoch);
    let bytes =
        serde_json::to_vec_pretty(&file).map_err(|e| MaccError::Validation(e.to_string()))?;
    crate::atomic_write(paths, &destination, &bytes)
}

pub(super) fn historical_annotation(
    paths: &ProjectPaths,
    past: &CoordinatorRunHistoryItem,
) -> Option<SupervisorAnnotation> {
    let bytes = fs::read(path(paths, &past.run_id, past.epoch)).ok()?;
    let file: IncidentFile = serde_json::from_slice(&bytes).ok()?;
    (file.run_id == past.run_id && file.epoch == past.epoch).then_some(file.supervisor)
}

impl CoordinatorRunSummary {
    pub fn previous_diagnosis(&self) -> Option<(&str, &SupervisorAnnotation)> {
        self.recent_runs
            .iter()
            .skip(1)
            .find_map(|run| Some((run.run_id.as_str(), run.supervisor.as_ref()?)))
    }

    pub fn refresh_supervisor(&mut self, paths: &ProjectPaths) {
        let file = match fs::read(path(paths, &self.run_id, self.epoch)) {
            Ok(raw) => serde_json::from_slice::<IncidentFile>(&raw).ok(),
            // Compatibility for runs diagnosed before per-run publication existed.
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    && !matches!(self.status.as_str(), "running" | "draining") =>
            {
                fs::read(paths.root.join(".macc/log/supervisor/latest.json"))
                    .ok()
                    .and_then(|raw| serde_json::from_slice(&raw).ok())
                    .and_then(|report| IncidentFile::from_report(&report))
            }
            Err(_) => None,
        };
        if let Some(file) = file {
            let note = file.annotation(self);
            if matches!(self.status.as_str(), "running" | "draining") {
                // The engine may have died before it could persist a terminal status.
                // Only its own native incident may report that observation.
                if note.is_none() || file.run_status.as_deref() != Some("crashed") {
                    return;
                }
                self.status = "crashed".into();
                self.severity = "error".into();
                self.headline = "Coordinator process stopped unexpectedly.".into();
                self.cause = file.reason;
                self.next_action = "Inspect the supervisor diagnosis and recovery outcome.".into();
            }
            self.supervisor = note;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supervisor_annotations_belong_to_one_run_epoch_and_task_attempt() {
        let mut task = Task {
            id: "A".into(),
            state: "blocked".into(),
            ..Default::default()
        };
        task.task_runtime.claim_id = Some("first".into());
        let summary = CoordinatorRunSummary {
            run_id: "run".into(),
            epoch: 3,
            task_id: Some("A".into()),
            task_revision: Some(crate::coordinator::control::task_revision(&task).unwrap()),
            ..Default::default()
        };
        let mut report = serde_json::json!({"intervention":{"incident":{"epoch":3,"run_id":"run","tasks":[task]},"status":"escalated","diagnosis":{"summary":"Look at the retained file."},"next_action":"Inspect the archive"}});
        assert_eq!(
            IncidentFile::from_report(&report)
                .unwrap()
                .annotation(&summary)
                .unwrap()
                .next_action
                .as_deref(),
            Some("Inspect the archive")
        );
        let root = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(root.path());
        let mut pinned = summary.clone();
        publish_incident(&paths, &report).unwrap();
        pinned.refresh_supervisor(&paths);
        assert_eq!(pinned.supervisor.as_ref().unwrap().status, "escalated");
        report["intervention"]["status"] = "recovered".into();
        report["intervention"]["detail"] = "Coordinator restarted".into();
        publish_incident(&paths, &report).unwrap();
        let mut other_run = report.clone();
        other_run["intervention"]["incident"]["run_id"] = "other".into();
        publish_incident(&paths, &other_run).unwrap();
        pinned.refresh_supervisor(&paths);
        let note = pinned.supervisor.unwrap();
        assert_eq!(note.status, "recovered");
        assert_eq!(note.outcome.as_deref(), Some("Coordinator restarted"));
        let mut unrelated = report.clone();
        unrelated["intervention"]["diagnosis"]["task_ids"] = serde_json::json!(["B"]);
        assert!(IncidentFile::from_report(&unrelated)
            .unwrap()
            .annotation(&summary)
            .is_none());
        let mut run_wide = summary.clone();
        run_wide.task_id = None;
        run_wide.task_revision = None;
        assert_eq!(
            IncidentFile::from_report(&report)
                .unwrap()
                .annotation(&run_wide)
                .unwrap()
                .task_ids,
            ["A"]
        );
        let mut orphan = run_wide.clone();
        orphan.status = "running".into();
        report["intervention"]["incident"]["run_status"] = "crashed".into();
        publish_incident(&paths, &report).unwrap();
        orphan.refresh_supervisor(&paths);
        assert_eq!(orphan.status, "crashed");
        assert_eq!(orphan.supervisor.unwrap().status, "recovered");
        let mut engine_report = report.clone();
        engine_report["intervention"]["incident"]["tasks"] = serde_json::json!([]);
        assert!(IncidentFile::from_report(&engine_report)
            .unwrap()
            .annotation(&run_wide)
            .is_some());
        report["intervention"]["incident"]["epoch"] = 2.into();
        assert!(IncidentFile::from_report(&report)
            .unwrap()
            .annotation(&summary)
            .is_none());
        report["intervention"]["incident"]["epoch"] = 3.into();
        report["intervention"]["incident"]["tasks"][0]["task_runtime"]["claim_id"] =
            "second".into();
        assert!(IncidentFile::from_report(&report)
            .unwrap()
            .annotation(&summary)
            .is_none());
    }
}

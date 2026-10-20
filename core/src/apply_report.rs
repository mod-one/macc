//! Human summaries and durable machine-readable Apply evidence.
use crate::{plan::ActionStatus, ApplyReport, ProjectPaths, Result};
impl ApplyReport {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn render_cli(&self) -> String {
        let counts = self.file_counts();
        let mut text = format!(
            "{}\n\nConfiguration  {} created, {} updated, {} unchanged\n",
            if self.warnings.is_empty() {
                "Apply completed"
            } else {
                "Apply completed with warnings"
            },
            counts.0,
            counts.1,
            counts.2
        );
        if self.outcomes.keys().any(|k| k.starts_with("O2:")) {
            text.push_str("O2 storage     Ready · migration/upgrade completed\n");
        }
        if self.outcomes.keys().any(|k| k.starts_with("Preparation:")) {
            text.push_str("Preparation    Verified\n");
        }
        for warning in &self.warnings {
            text.push_str(&format!("WARN  {warning}\n"));
        }
        if let Some(path) = &self.backup_dir {
            text.push_str(&format!("Backups: {}\n", path.display()));
        }
        if let Some(report) = &self.user_backup_report {
            text.push_str(&format!("User backups: {}\n", report.root.display()));
        }
        if let Some(path) = &self.details_path {
            text.push_str(&format!("Details: {}\n", path.display()));
        }
        text
    }
    pub fn file_counts(&self) -> (usize, usize, usize) {
        let mut counts = (0, 0, 0);
        for (path, status) in &self.outcomes {
            if ["O2:", "Preparation:", "Prepare:"]
                .iter()
                .any(|p| path.starts_with(p))
            {
                continue;
            }
            match status {
                ActionStatus::Created => counts.0 += 1,
                ActionStatus::Updated => counts.1 += 1,
                ActionStatus::Unchanged => counts.2 += 1,
                _ => {}
            }
        }
        counts
    }
    pub fn render_detailed(&self) -> String {
        let mut text = self.render_cli();
        for (path, status) in &self.outcomes {
            text.push_str(&format!("{:<10} {path}\n", status.as_str()));
        }
        text
    }
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({"schema_version":1,"status":if self.warnings.is_empty(){"completed"}else{"completed_with_warnings"},"files":{"created":self.file_counts().0,"updated":self.file_counts().1,"unchanged":self.file_counts().2},"outcomes":self.outcomes.iter().map(|(p,s)|(p.clone(),s.as_str())).collect::<std::collections::BTreeMap<_,_>>(),"warnings":self.warnings,"details_path":self.details_path,"backup_dir":self.backup_dir,"user_backup_dir":self.user_backup_report.as_ref().map(|r|&r.root)})
    }
    pub(crate) fn save_details(
        &mut self,
        paths: &ProjectPaths,
        run_id: Option<&str>,
    ) -> Result<()> {
        let directory = paths.macc_dir.join("log/apply").join(
            run_id
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        );
        self.details_path = Some(directory.join("result.json"));
        crate::atomic_write(
            paths,
            self.details_path.as_ref().unwrap(),
            &serde_json::to_vec_pretty(&self.json())
                .map_err(|e| crate::MaccError::Validation(e.to_string()))?,
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summary_hides_unchanged_files_and_separates_preparation_from_file_updates() {
        let mut report = ApplyReport::new();
        for i in 0..437 {
            report
                .outcomes
                .insert(format!("file-{i}"), ActionStatus::Unchanged);
        }
        report
            .outcomes
            .insert("O2: migrate".into(), ActionStatus::Updated);
        report
            .warnings
            .push("Native index has partial coverage".into());
        assert_eq!(report.file_counts(), (0, 0, 437));
        assert!(report.render_cli().lines().count() < 12);
        assert!(!report.render_cli().contains("file-436"));
        assert!(report.render_detailed().contains("file-436"));
        assert_eq!(report.json()["status"], "completed_with_warnings");
    }
}

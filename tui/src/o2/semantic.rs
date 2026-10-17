//! Guided semantic benchmark/review operations use the shared application port.
use super::*;
impl Panel {
    pub(super) fn semantic_result(&mut self, value: &Value) {
        let envelope = if value["report"]["id"].is_string() {
            Some(value)
        } else {
            value["reports"].as_array().and_then(|rows| rows.first())
        };
        if let Some(report) = envelope {
            self.semantic_report = Some(report.clone());
            self.semantic_scope = self.access.clone();
        }
    }
    pub(super) fn semantic_key(&mut self, root: &Path, key: KeyCode) -> Result<bool> {
        if key == KeyCode::Char('a') {
            let valid = match self.pending.as_ref() {
                Some(Action::SemanticGate {
                    report_id,
                    checksum,
                    ..
                }) => Some(
                    self.tab == 0
                        && self.semantic_scope == self.access
                        && self.semantic_report.as_ref().is_some_and(|r| {
                            r["report"]["id"].as_str() == Some(report_id)
                                && r["checksum"].as_str() == Some(checksum)
                        }),
                ),
                Some(Action::SemanticBenchmark { suite }) => {
                    let access: macc_core::o2::memory::Access =
                        serde_json::from_value(self.access.clone())
                            .map_err(|e| MaccError::Validation(e.to_string()))?;
                    Some(
                        self.tab == 0
                            && serde_json::to_value(&access).ok()
                                == serde_json::to_value(&suite.access).ok(),
                    )
                }
                _ => None,
            };
            if valid == Some(false) {
                self.pending = None;
                return Err(MaccError::Validation(
                    "Semantic review scope/report changed; inspect and prepare the operation again"
                        .into(),
                ));
            }
        }
        if self.tab != 0 {
            return Ok(false);
        }
        match key {
            KeyCode::Char('S') => self.launch(
                root,
                Action::SemanticStatus {
                    access: serde_json::from_value(self.access.clone())
                        .map_err(|e| MaccError::Validation(e.to_string()))?,
                },
            )?,
            KeyCode::Char('Q') => self.start(
                "semantic_search",
                false,
                &["Approved fact query (FTS plus benchmark-gated local semantics)"],
            ),
            KeyCode::Char('B') => {
                self.pending = None;
                self.start(
                    "semantic_size",
                    true,
                    &["Number of representative queries (4–100; blank selects 4)"],
                )
            }
            KeyCode::Char(mode @ ('G' | 'X')) => {
                let r = self
                    .semantic_report
                    .as_ref()
                    .filter(|_| self.semantic_scope == self.access)
                    .ok_or_else(|| {
                        MaccError::Validation(
                            "Run B or inspect S for a benchmark in this scope first".into(),
                        )
                    })?;
                if mode == 'G' && (r["report"]["passed"] != true || r["approvable"] == false) {
                    return Err(MaccError::Validation(
                        "Run a passing benchmark for the current corpus, settings and recipe before approval".into(),
                    ));
                }
                let action = Action::SemanticGate {
                    report_id: r["report"]["id"].as_str().unwrap_or_default().into(),
                    checksum: r["checksum"].as_str().unwrap_or_default().into(),
                    enabled: mode == 'G',
                    confirmed: true,
                };
                self.details=format!("Inspect representative labels, quality, resources and exact report before confirming:\n{}\n{}",serde_json::to_string_pretty(r).unwrap_or_default(),serde_json::to_string_pretty(&action).unwrap_or_default());
                self.pending = Some(action);
                self.message =
                    "Reviewed semantic gate prepared; press a to confirm or Esc to cancel.".into();
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

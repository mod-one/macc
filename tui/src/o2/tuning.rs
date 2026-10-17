//! Dedicated guided policy-trial workflow; core owns statistics and permissions.
use super::*;
use macc_core::o2::tuning;
impl Panel {
    pub(super) fn tuning_key(&mut self, _root: &Path, key: KeyCode) -> Result<bool> {
        if key == KeyCode::Char('a') {
            if let Some(Action::TuningApply {
                report_id,
                checksum,
                revision,
                ..
            }) = &self.pending
            {
                if self.tab != 6
                    || !self.tuning_preview.as_ref().is_some_and(|p| {
                        p["report_id"].as_str() == Some(report_id)
                            && p["checksum"].as_str() == Some(checksum)
                            && p["revision"].as_str() == Some(revision)
                    })
                {
                    self.pending = None;
                    return Err(MaccError::Validation(
                        "Policy trial preview changed; inspect and prepare adoption again".into(),
                    ));
                }
            }
        }
        if self.tab != 6 {
            if matches!(
                self.pending,
                Some(
                    Action::TuningCreate { .. }
                        | Action::TuningObserve { .. }
                        | Action::TuningClose { .. }
                        | Action::TuningApply { .. }
                )
            ) && key == KeyCode::Char('a')
            {
                self.pending = None;
                return Err(MaccError::Validation(
                    "Return to Policy trials and prepare the operation again".into(),
                ));
            }
            return Ok(false);
        }
        match key {
            KeyCode::Left=>self.tuning_field=self.tuning_field.saturating_sub(1),
            KeyCode::Right=>self.tuning_field=(self.tuning_field+1).min(tuning::FIELDS.len()-1),
            KeyCode::Char('n')=>self.start("tuning_create",true,&["Candidate value for the selected safe field (Left/Right changes field)","Distinct representative task IDs (comma-separated; at least configured minimum)","Full starting Git revision for both arms","Protocol file path (checks, representative sample, controlled conditions)","Exact SHA-256 of the inspected protocol"]),
            KeyCode::Char('i')=>self.start("tuning_inspect",false,&["Trial ID (blank lists trials)"]),
            KeyCode::Char('q')=>self.start("tuning_observe",true,&["Trial ID","Task ID","Baseline accepted (true/false)","Baseline tests passed (true/false)","Baseline specification fidelity (true/false)","Baseline maximum severity (0 none, 4 critical)","Baseline retry count","Candidate accepted (true/false)","Candidate tests passed (true/false)","Candidate specification fidelity (true/false)","Candidate maximum severity (0–4)","Candidate retry count","Quality evidence file path","Exact SHA-256 of the inspected quality evidence"]),
            KeyCode::Char('e')=>self.start("tuning_close",true,&["Trial ID to evaluate (all registered pairs required)"]),
            KeyCode::Char('x')=>self.start("tuning_cancel",true,&["Trial ID to cancel (records and alpha allocation retained)"]),
            KeyCode::Char('p')=>self.start("tuning_preview",false,&["Passing report ID","Exact inspected report checksum"]),
            KeyCode::Char('s')=>{
                let v=self.tuning_preview.as_ref().ok_or_else(||MaccError::Validation("Preview a passing report with p first".into()))?;
                let action=Action::TuningApply {report_id:v["report_id"].as_str().unwrap_or_default().into(),checksum:v["checksum"].as_str().unwrap_or_default().into(),revision:v["revision"].as_str().unwrap_or_default().into(),confirmed:true};
                self.details=serde_json::to_string_pretty(v).unwrap_or_default();self.pending=Some(action);self.message="Inspect the policy diff; a confirms adoption while stopped.".into();
            }
            _=>return Ok(false),
        }
        Ok(true)
    }
    pub(super) fn tuning_form(&mut self, root: &Path, name: &str, v: &[String]) -> Result<bool> {
        if !name.starts_with("tuning_") {
            return Ok(false);
        }
        let parse = |s: &str| {
            s.parse::<u64>()
                .map_err(|_| MaccError::Validation("Enter a nonnegative whole number".into()))
        };
        let quality = |offset: usize| -> Result<tuning::Quality> {
            Ok(tuning::Quality {
                accepted: v[offset]
                    .parse()
                    .map_err(|_| MaccError::Validation("Enter true or false".into()))?,
                tests_passed: v[offset + 1]
                    .parse()
                    .map_err(|_| MaccError::Validation("Enter true or false".into()))?,
                spec_fidelity: v[offset + 2]
                    .parse()
                    .map_err(|_| MaccError::Validation("Enter true or false".into()))?,
                max_finding_severity: u8::try_from(parse(&v[offset + 3])?)
                    .map_err(|_| MaccError::Validation("Severity must be 0–4".into()))?,
                retries: parse(&v[offset + 4])?,
            })
        };
        let action = match name {
            "tuning_create" => Action::TuningCreate {
                plan: tuning::Plan {
                    field: tuning::FIELDS[self.tuning_field].into(),
                    candidate: v[0].parse().map_err(|_| {
                        MaccError::Validation("Enter a numeric candidate value".into())
                    })?,
                    cases: v[1]
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|s| tuning::Case {
                            task_id: s.into(),
                            revision: v[2].clone(),
                        })
                        .collect(),
                    protocol_path: v[3].clone(),
                    protocol_sha256: v[4].clone(),
                },
                confirmed: true,
            },
            "tuning_inspect" => Action::TuningStatus {
                id: (!v[0].is_empty()).then(|| v[0].clone()),
            },
            "tuning_observe" => Action::TuningObserve {
                trial_id: v[0].clone(),
                observation: tuning::Observation {
                    task_id: v[1].clone(),
                    baseline: quality(2)?,
                    candidate: quality(7)?,
                    evidence_path: v[12].clone(),
                    evidence_sha256: v[13].clone(),
                },
                confirmed: true,
            },
            "tuning_close" | "tuning_cancel" => Action::TuningClose {
                trial_id: v[0].clone(),
                cancel: name == "tuning_cancel",
                confirmed: true,
            },
            "tuning_preview" => Action::TuningPreview {
                report_id: v[0].clone(),
                checksum: v[1].clone(),
            },
            _ => return Err(MaccError::Validation("Unknown policy trial form".into())),
        };
        self.tuning_preview = None;
        self.pending = None;
        if action.is_mutation() {
            self.details = format!(
                "Review this exact policy-trial operation:\n{}",
                serde_json::to_string_pretty(&action).unwrap_or_default()
            );
            self.pending = Some(action);
            self.message = "Prepared; a confirms. No policy or trial has changed yet.".into();
        } else {
            self.launch(root, action)?;
        }
        Ok(true)
    }
}

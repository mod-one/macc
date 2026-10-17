//! Variable-size guided suites; no raw JSON is needed for larger benchmarks.
use super::*;
use macc_core::o2::semantic::{Case, Suite};

impl Panel {
    pub(super) fn semantic_form(&mut self, name: &str, values: &[String]) -> Result<bool> {
        if name == "semantic_size" {
            let count = if values[0].trim().is_empty() {
                4
            } else {
                values[0].parse::<usize>().map_err(|_| {
                    MaccError::Validation("Enter a query count from 4 to 100".into())
                })?
            };
            if !(4..=100).contains(&count) {
                return Err(MaccError::Validation(
                    "Enter a query count from 4 to 100".into(),
                ));
            }
            let mut labels = Vec::with_capacity(count * 2 + 1);
            for i in 1..=count {
                labels.push(format!("Representative query {i} of {count}"));
                labels.push(format!(
                    "Relevant reviewed memory IDs {i} (comma-separated)"
                ));
            }
            labels.push("Top k (1–10; measured results also obey Memory search limit)".into());
            self.start(
                "semantic_benchmark",
                true,
                &labels.iter().map(String::as_str).collect::<Vec<_>>(),
            );
            return Ok(true);
        }
        if name != "semantic_benchmark" {
            return Ok(false);
        }
        let count = values.len().saturating_sub(1) / 2;
        if !(4..=100).contains(&count) || values.len() != count * 2 + 1 {
            return Err(MaccError::Validation(
                "Benchmark needs 4–100 query/label pairs".into(),
            ));
        }
        let top_k = values[count * 2]
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=10).contains(n))
            .ok_or_else(|| MaccError::Validation("Enter top k from 1 to 10".into()))?;
        let cases = values[..count * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|row| Case {
                query: row[0].trim().into(),
                relevant_ids: row[1]
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect(),
            })
            .collect::<Vec<_>>();
        if cases
            .iter()
            .any(|c| c.query.is_empty() || c.relevant_ids.is_empty())
        {
            return Err(MaccError::Validation(
                "Every query needs text and reviewed memory IDs".into(),
            ));
        }
        let action = Action::SemanticBenchmark {
            suite: Suite {
                access: serde_json::from_value(self.access.clone())
                    .map_err(|e| MaccError::Validation(e.to_string()))?,
                cases,
                top_k,
            },
        };
        self.details = format!(
            "Review this exact operation before pressing a:\n{}",
            serde_json::to_string_pretty(&action).unwrap_or_default()
        );
        self.pending = Some(action);
        self.message =
            "Prepared labelled benchmark; a confirms, Esc closes the form without running it."
                .into();
        Ok(true)
    }
}

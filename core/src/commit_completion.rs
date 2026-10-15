//! Completion evidence is stricter than merely mentioning a task in a commit.
use crate::commit_message::{self, TAG_TASK};
use regex::Regex;
use std::sync::OnceLock;

/// Keep tolerant parsing for inspection, while excluding explicit unfinished work
/// from reconciliation, branch sync and cross-lot dependency satisfaction.
pub fn task_id(raw: &str) -> Option<String> {
    let parsed = commit_message::parse(raw);
    if is_unfinished(&parsed) {
        return None;
    }
    // An arbitrary occurrence of an ID is context, not proof of delivery.
    if !parsed.tags.contains_key(TAG_TASK)
        && parsed.message.is_none()
        && !parsed
            .subject
            .to_ascii_lowercase()
            .starts_with("macc: merge task ")
    {
        return None;
    }
    parsed.task_id
}

fn is_unfinished(parsed: &commit_message::ParsedCommit) -> bool {
    static CHECKPOINT: OnceLock<Regex> = OnceLock::new();
    let checkpoint = CHECKPOINT.get_or_init(|| Regex::new(
        r"(?i)^(?:\[?(?:wip|draft|partial|checkpoint)\]?(?:\([^)]*\))?(?:[\s:!]|$)|fixup!|squash!|revert(?:[\s:]|$))"
    ).expect("valid checkpoint prefix"));
    checkpoint.is_match(&parsed.subject)
        || parsed
            .message
            .as_ref()
            .and_then(|message| message.title.as_deref())
            .is_some_and(|title| checkpoint.is_match(title))
        || parsed.tags.get("phase").is_some_and(|phase| {
            matches!(
                phase.to_ascii_lowercase().as_str(),
                "wip" | "draft" | "partial" | "checkpoint"
            )
        })
        || parsed
            .tags
            .get("wip")
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
        || parsed
            .tags
            .get("validation")
            .is_some_and(|value| value != "true")
        || parsed.tags.get("result").is_some_and(|result| {
            !matches!(
                result.as_str(),
                "success_with_changes" | "success_without_changes" | "already_satisfied"
            )
        })
}

/// Input is newest first, as returned by Git. A checkpoint is superseded only
/// by a later delivery of the same task; callers must not merge a mixed branch.
pub fn unfinished_task_ids<'a>(messages: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut latest = std::collections::BTreeMap::new();
    for message in messages {
        let parsed = commit_message::parse(message);
        let completed = task_id(message).is_some();
        if completed || is_unfinished(&parsed) {
            if let Some(id) = parsed.task_id {
                latest.entry(id).or_insert(completed);
            }
        }
    }
    latest
        .into_iter()
        .filter_map(|(id, complete)| (!complete).then_some(id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkpoints_never_complete_a_task_even_with_a_task_trailer() {
        for subject in [
            "wip: TASK-001 implementation",
            "WIP(native): TASK-001",
            "feat: TASK-001 - WIP implementation",
            "[WIP] TASK-001",
            "draft: TASK-001",
            "partial: TASK-001",
            "checkpoint: TASK-001",
            "fixup! feat: TASK-001",
            "squash! feat: TASK-001",
            "Revert TASK-001",
        ] {
            assert_eq!(task_id(subject), None, "{subject}");
            assert_eq!(
                task_id(&format!("{subject}\n\n[macc:task TASK-001]")),
                None,
                "{subject}"
            );
        }
    }
    #[test]
    fn successful_legacy_and_validation_commits_remain_compatible() {
        for message in ["feat: TASK-001 - delivered", "macc: merge task TASK-001", "delivered\n\n[macc:task TASK-001]", "chore: TASK-001 - Validate\n\n[macc:task TASK-001]\n[macc:result already_satisfied]\n[macc:validation true]"] {
            assert_eq!(task_id(message), Some("TASK-001".into()), "{message}");
        }
        assert_eq!(task_id("investigate TASK-001 failure"), None);
    }
    #[test]
    fn merge_subject_mentions_do_not_create_unfinished_task_owners() {
        assert!(unfinished_task_ids([
            "Merge branch 'ai/test-tool/worker-01'",
            "feat: TASK-001 - delivered"
        ])
        .is_empty());
        assert_eq!(
            unfinished_task_ids(["wip: TASK-001", "feat: TASK-001 - delivered"]),
            vec!["TASK-001"]
        );
        assert!(unfinished_task_ids(["feat: TASK-001 - delivered", "wip: TASK-001"]).is_empty());
    }
    #[test]
    fn negative_results_override_positive_subjects() {
        for marker in [
            "[macc:result precondition_unmet]",
            "[macc:result error_with_changes]",
            "[macc:validation false]",
            "[macc:phase checkpoint]",
            "[macc:wip true]",
        ] {
            assert_eq!(
                task_id(&format!("feat: TASK-001\n\n[macc:task TASK-001]\n{marker}")),
                None
            );
        }
    }
}

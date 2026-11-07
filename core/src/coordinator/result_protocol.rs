//! Terminal output hygiene shared by completion attribution and legacy readers.
pub fn valid_explanation(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && ![
            "<reason>",
            "<condition>",
            "<evidence>",
            "<summary>",
            "<explanation>",
            "<result>",
            "<kind>",
            "<status>",
        ]
        .iter()
        .any(|p| value.to_ascii_lowercase().contains(p))
}
/// Exclude instruction echoes, quoted examples and command transcripts. Native
/// runner summaries remain eligible; they are not provider compliance evidence.
pub fn projection(raw: &str) -> String {
    let (mut prompt, mut fence, mut transcript) = (false, false, false);
    let mut out = String::new();
    for line in raw.lines() {
        if line.starts_with("MACC_PROMPT_BEGIN:") {
            prompt = true;

            continue;
        }
        if line.starts_with("MACC_PROMPT_END:") {
            prompt = false;
            continue;
        }
        if prompt {
            continue;
        }
        if line.trim_start().starts_with("```") {
            fence = !fence;
            continue;
        }
        if fence {
            continue;
        }
        match line {
            "exec" | "tool" | "user" | "system" | "developer" => {
                transcript = true;
                continue;
            }
            "codex" | "assistant" | "final" => {
                transcript = false;
                continue;
            }
            _ => {}
        }
        if transcript {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("MACC_TASK_") || trimmed.starts_with("- Explanation:") {
            if !valid_explanation(trimmed) {
                continue;
            }
            if line.starts_with(char::is_whitespace) {
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}
/// A completion cannot borrow an explanation from an earlier prose/transcript block.
pub fn completion_projection(raw: &str) -> String {
    let current = raw
        .rfind("MACC_PROMPT_BEGIN:")
        .map(|i| &raw[i..])
        .unwrap_or(raw);
    let projected = projection(current);
    let mut fields = Vec::new();
    let mut boundary = false;
    for line in projected.lines() {
        let is_field = line.starts_with("MACC_TASK_")
            || line.starts_with("- Result kind:")
            || line.starts_with("- Explanation:")
            || line.starts_with("- Unmet precondition:");
        if is_field {
            if boundary {
                fields.clear();
                boundary = false;
            }
            fields.push(line)
        } else if !fields.is_empty() {
            boundary = true
        }
    }
    let markers: Vec<_> = fields
        .iter()
        .enumerate()
        .filter_map(|(i, line)| line.starts_with("MACC_TASK_RESULT:").then_some(i))
        .collect();
    if markers.len() > 1 {
        fields.drain(..=markers[markers.len() - 2]);
    }
    fields.join("\n")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn echoed_instruction_is_not_a_result() {
        let s="MACC_PROMPT_BEGIN: instructions-only\nMACC_TASK_RESULT_EXP: prior work unsalvageable: <reason>\nMACC_TASK_RESULT: error_without_changes\nMACC_PROMPT_END: instructions-only\ncodex\nMACC_TASK_RESULT: success_with_changes\n";
        let p = projection(s);
        assert!(!p.contains("unsalvageable"));
        assert!(!p.contains("error_without_changes"));
        assert!(p.contains("success_with_changes"));
        assert!(!valid_explanation("prior work unsalvageable: <reason>"));
    }
    #[test]
    fn older_attempt_is_not_a_fallback_for_a_new_invocation() {
        assert!(!completion_projection("MACC_TASK_RESULT: error_without_changes\nMACC_PROMPT_BEGIN: x\nold instructions\nMACC_PROMPT_END: x\nno result").contains("error_without_changes"));
    }
    #[test]
    fn shell_and_native_terminal_projection_agree_on_transcripts() {
        use std::process::Command;
        let cases = [
            "MACC_TASK_RESULT_EXP: old reason\nMACC_TASK_RESULT: error_without_changes\nMACC_TASK_RESULT: success_with_changes\n",
            "MACC_PROMPT_BEGIN: x\nMACC_TASK_RESULT_EXP: prior work unsalvageable: <reason>\nMACC_TASK_RESULT: error_without_changes\nMACC_PROMPT_END: x\ncodex\nMACC_TASK_RESULT: success_with_changes\n",
            "MACC_TASK_RESULT: error_without_changes\nMACC_TASK_RESULT_EXP: old reason\n\nAnother invocation completed.\nMACC_TASK_RESULT: success_with_changes\n",
            "```text\nMACC_TASK_RESULT: error_without_changes\n```\nMACC_TASK_RESULT_EXP: valid evidence\nMACC_TASK_RESULT: already_satisfied\n",
        ];
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("output.txt");
        for raw in cases {
            std::fs::write(&file, raw).unwrap();
            let helper = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../automat/result_protocol.sh");
            let o = Command::new("bash")
                .args(["-c", r#"source "$1"; terminal_result_lines "$2""#, "test"])
                .arg(helper)
                .arg(&file)
                .output()
                .unwrap();
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
            assert_eq!(
                String::from_utf8_lossy(&o.stdout).trim(),
                completion_projection(raw).trim()
            );
        }
    }
}

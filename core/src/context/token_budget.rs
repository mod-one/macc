const CHARS_PER_TOKEN: usize = 4;

/// Approximation used consistently for complete serialized context payloads.
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(CHARS_PER_TOKEN)
}
/// Bound UTF-8 text including an optional framing suffix, without splitting a code point.
pub fn bounded_with_suffix(
    text: &str,
    max_tokens: usize,
    max_bytes: usize,
    suffix: &str,
) -> (String, bool) {
    let chars = max_tokens.saturating_mul(CHARS_PER_TOKEN);
    if text.chars().count() <= chars && text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    let mut ending = String::new();
    for c in suffix.chars().take(chars) {
        if ending.len() + c.len_utf8() > max_bytes {
            break;
        }
        ending.push(c);
    }
    let mut out = String::new();
    let remaining = chars.saturating_sub(ending.chars().count());
    for c in text.chars().take(remaining) {
        if out.len() + c.len_utf8() + ending.len() > max_bytes {
            break;
        }
        out.push(c);
    }
    out.push_str(&ending);
    (out, true)
}
/// Enforce the estimate budget on the complete returned text, including its notice.
pub fn enforce_budget(text: &str, max_tokens: usize) -> (String, bool) {
    bounded_with_suffix(
        text,
        max_tokens,
        max_tokens.saturating_mul(CHARS_PER_TOKEN),
        "\n[Output truncated; inspect raw evidence]",
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_and_notice_share_the_budget() {
        for budget in 0..40 {
            for text in ["€".repeat(100), "plain ASCII ".repeat(100)] {
                let (out, truncated) = enforce_budget(&text, budget);
                assert!(truncated);
                assert!(estimate_tokens(&out) <= budget);
                let (out, _) = bounded_with_suffix(&text, budget, 50, "\n[raw]");
                assert!(out.len() <= 50);
                assert!(estimate_tokens(&out) <= budget);
            }
        }
    }
}

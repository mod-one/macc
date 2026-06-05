const CHARS_PER_TOKEN: usize = 4;

/// Enforce a token budget on text. Returns (text, was_truncated).
/// Approximation: 1 token ≈ 4 characters.
pub fn enforce_budget(text: &str, max_tokens: usize) -> (String, bool) {
    let max_chars = max_tokens * CHARS_PER_TOKEN;
    if text.len() <= max_chars {
        return (text.to_string(), false);
    }
    let truncated = &text[..max_chars];
    let last_newline = truncated.rfind('\n').unwrap_or(max_chars);
    let truncated = &text[..last_newline];
    (
        format!(
            "{}\n\n[Output truncated: {} tokens used, {} token budget]",
            truncated,
            text.len() / CHARS_PER_TOKEN,
            max_tokens
        ),
        true,
    )
}

pub fn estimate_tokens(text: &str) -> usize {
    text.len() / CHARS_PER_TOKEN
}

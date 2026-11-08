//! Provider-neutral projection of assistant protocol and native runner summaries.
mod provider;
pub use provider::provider_output;

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
#[derive(Clone, Copy, Default, PartialEq)]
enum Channel {
    #[default]
    Provider,
    Assistant,
    Transcript,
}
#[derive(Default)]
struct State {
    prompt: bool,
    channel: Channel,
    fence: Option<(char, usize)>,
}
impl State {
    fn line(&mut self, line: &str) -> bool {
        if line.starts_with("MACC_PROMPT_BEGIN:") {
            self.prompt = true;
            self.fence = None;
            return false;
        }
        if line.starts_with("MACC_PROMPT_END:") {
            self.prompt = false;
            self.channel = Channel::Provider;
            self.fence = None;
            return false;
        }
        if self.prompt {
            return false;
        }
        // Channel boundaries must be handled before Markdown. Tool output is
        // excluded without touching the assistant's Markdown state.
        let channel = match line {
            "exec" | "tool" | "user" | "system" | "developer" => Some(Channel::Transcript),
            "codex" | "assistant" | "final" => Some(Channel::Assistant), // macc:allow-tool-name: emitted transcript role token, never adapter routing.
            _ => None,
        };
        if let Some(channel) =
            channel.filter(|_| self.channel == Channel::Transcript || self.fence.is_none())
        {
            self.channel = channel;
            self.fence = None;
            return false;
        }
        if self.channel == Channel::Transcript {
            return false;
        }
        let trimmed = line.trim_start();
        if let Some(ch) = trimmed.chars().next().filter(|c| matches!(c, '`' | '~')) {
            let len = trimmed.chars().take_while(|c| *c == ch).count();
            if len >= 3 {
                match self.fence {
                    None => self.fence = Some((ch, len)),
                    Some((kind, size))
                        if kind == ch && len >= size && trimmed[len..].trim().is_empty() =>
                    {
                        self.fence = None
                    }
                    _ => {}
                }
                return false;
            }
        }
        self.fence.is_none() && !line.trim_start().starts_with('>')
    }
}
/// Prompt echoes, tool transcripts and fenced examples cannot provide protocol.
pub fn projection(raw: &str) -> String {
    let mut state = State::default();
    let mut out = String::new();
    for line in raw.lines() {
        if !state.line(line) {
            continue;
        }
        let trimmed = line.trim();
        if (trimmed.starts_with("MACC_")
            || trimmed.starts_with("REVIEW_VERDICT:")
            || trimmed.starts_with("- Explanation:"))
            && (!valid_explanation(trimmed) || line.starts_with(char::is_whitespace))
        {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}
/// A terminal result cannot borrow an explanation from an older result block.
pub fn completion_projection(raw: &str) -> String {
    let current = raw
        .rfind("MACC_PROMPT_BEGIN:")
        .map(|i| &raw[i..])
        .unwrap_or(raw);
    let projected = projection(current);
    let mut fields = Vec::new();
    let mut boundary = false;
    for line in projected.lines() {
        if line.starts_with("MACC_TASK_")
            || ["- Result kind:", "- Explanation:", "- Unmet precondition:"]
                .iter()
                .any(|p| line.starts_with(p))
        {
            if boundary {
                fields.clear();
                boundary = false;
            }
            fields.push(line);
        } else if !fields.is_empty() {
            boundary = true;
        }
    }
    let markers: Vec<_> = fields
        .iter()
        .enumerate()
        .filter_map(|(i, l)| l.starts_with("MACC_TASK_RESULT:").then_some(i))
        .collect();
    if markers.len() > 1 {
        fields.drain(..=markers[markers.len() - 2]);
    }
    fields.join("\n")
}
#[cfg(test)]
mod tests;
